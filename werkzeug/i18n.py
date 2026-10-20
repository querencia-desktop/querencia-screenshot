#!/usr/bin/env python3
"""po-Pflege für Querencia-Programme, unabhängig von GNU gettext.

Ersatz für xgettext/msgmerge/msgfmt (entstanden, als das Paket `gettext`
auf der Entwicklungs-VM fehlte; xgettext liest Rust erst ab 0.24). Abgleich
mit gettext 0.23.2: .mo inhaltsgleich, Desktop-Datei byte-gleich zu
msgfmt --desktop. Die Ausgaben sind mit
GNU gettext verträglich: .pot/.po im üblichen Format, .mo nach der
GNU-Spezifikation (Magic 0x950412de, Revision 0, sortierte Schlüssel,
Kontext als „ctxt\\x04msgid“, Pluralformen durch \\0 getrennt).

Aufruf (im Repo-Wurzelverzeichnis):
  i18n.py pot                     po/<domäne>.pot aus src/*.rs und data/*.desktop.in
  i18n.py update                  jede po/<lang>.po an die .pot anpassen (msgmerge ohne Fuzzy)
  i18n.py mo <ziel>               <ziel>/<lang>/LC_MESSAGES/<domäne>.mo für jede po/LINGUAS-Sprache
  i18n.py desktop <ein.in> <aus>  Desktop-Datei mit Name[lang]= usw. (wie msgfmt --desktop)
  i18n.py pruefen                 .pot aktuell, alle .po vollständig, Platzhalter und Zugriffstasten stimmen

Erkannte Aufrufe in Rust: gettext("…"), gettext_f("…", …), ngettext("…", "…", n),
ngettext_f("…", "…", n, …), pgettext("ctx", "…"). Ein Kommentar
`// TRANSLATORS: …` in der Zeile davor landet als Übersetzerhinweis in der .pot.
Mit GNU gettext ≥ 0.24 entspricht das
  xgettext -L Rust --from-code=UTF-8 --add-comments=TRANSLATORS \\
    -k -kgettext -kgettext_f -kngettext:1,2 -kngettext_f:1,2 -kpgettext:1c,2
"""

import glob
import os
import re
import struct
import sys

DOMAIN = "querencia-screenshot"
PACKAGE = "Querencia Screenshot"
DESKTOP_KEYS = ("Name", "GenericName", "Comment", "Keywords")
KEYWORDS = {  # Name -> (Zahl der Text-Argumente, Kontext zuerst?)
    "gettext": ("s", False),
    "gettext_f": ("s", False),
    "ngettext": ("p", False),
    "ngettext_f": ("p", False),
    "pgettext": ("c", True),
}
HEADER_PLURALS = {
    "de": "nplurals=2; plural=(n != 1);",
    "en": "nplurals=2; plural=(n != 1);",
    "fr": "nplurals=2; plural=(n > 1);",
}


# ------------------------------------------------------------------ Rust
def rust_string(text, i):
    """Liest ein Rust-Zeichenkettenliteral ab text[i] == '"'; liefert (Wert, Ende)."""
    assert text[i] == '"'
    i += 1
    out = []
    while True:
        c = text[i]
        if c == '"':
            return "".join(out), i + 1
        if c == "\\":
            n = text[i + 1]
            if n == "\n":  # Fortsetzungszeile: führende Leerzeichen entfallen
                i += 2
                while text[i] in " \t\n\r":
                    i += 1
                continue
            esc = {"n": "\n", "t": "\t", "r": "\r", "\\": "\\", '"': '"', "'": "'", "0": "\0"}
            if n in esc:
                out.append(esc[n])
                i += 2
                continue
            if n == "u":
                end = text.index("}", i)
                out.append(chr(int(text[i + 3:end], 16)))
                i = end + 1
                continue
            raise ValueError(f"unbekannte Escape-Folge \\{n}")
        out.append(c)
        i += 1


def skip_ws(text, i, comments=None):
    """Überspringt Leerraum und //-Kommentare (gesammelt in `comments`)."""
    while True:
        while text[i] in " \t\r\n":
            i += 1
        if text.startswith("//", i):
            end = text.index("\n", i)
            if comments is not None:
                comments.append(text[i + 2:end].strip())
            i = end
            continue
        return i


CALL = re.compile(r"(?<![\w:.])(?:i18n::)?(gettext_f|gettext|ngettext_f|ngettext|pgettext)\s*\(")


def extract_rust(path, rel):
    text = open(path, encoding="utf-8").read()
    lines = text.split("\n")
    found = []
    for m in CALL.finditer(text):
        name = m.group(1)
        # Definitionen (`fn gettext_f(`) und Kommentare überspringen
        line_start = text.rfind("\n", 0, m.start()) + 1
        prefix = text[line_start:m.start()]
        if prefix.lstrip().startswith("//") or re.search(r"\bfn\s+$", prefix) or "use " in prefix:
            continue
        inner = []
        i = skip_ws(text, m.end(), inner)
        if text[i] != '"':
            continue  # kein Literal (z. B. Durchreichen einer Variablen)
        parts = []
        need = 2 if KEYWORDS[name][0] in ("p", "c") else 1
        while len(parts) < need:
            i = skip_ws(text, i)
            if text[i] != '"':
                raise SystemExit(f"{rel}: {name}(…) ohne Literal an Argument {len(parts) + 1}")
            s, i = rust_string(text, i)
            parts.append(s)
            i = skip_ws(text, i)
            if len(parts) < need:
                if text[i] != ",":
                    raise SystemExit(f"{rel}: {name}(…) erwartet weiteres Argument")
                i += 1
        lineno = text.count("\n", 0, m.start()) + 1
        comment = []
        k = lineno - 2
        while k >= 0 and lines[k].strip().startswith("//"):
            comment.insert(0, lines[k].strip()[2:].strip())
            k -= 1
        note = None
        joined = " ".join(comment + inner)
        if "TRANSLATORS:" in joined:
            note = joined[joined.index("TRANSLATORS:"):]
        if KEYWORDS[name][0] == "c":
            entry = {"ctxt": parts[0], "id": parts[1]}
        elif KEYWORDS[name][0] == "p":
            entry = {"ctxt": None, "id": parts[0], "plural": parts[1]}
        else:
            entry = {"ctxt": None, "id": parts[0]}
        entry["ref"] = f"{rel}:{lineno}"
        entry["note"] = note
        if "{" in "".join(parts):
            entry["flags"] = ["no-c-format"]
        found.append(entry)
    return found


def extract_desktop(path, rel):
    found = []
    for lineno, line in enumerate(open(path, encoding="utf-8"), 1):
        m = re.match(r"^(%s)=(.*)$" % "|".join(DESKTOP_KEYS), line.rstrip("\n"))
        if m:
            found.append({"ctxt": None, "id": m.group(2), "ref": f"{rel}:{lineno}",
                          "note": f"TRANSLATORS: Desktop-Datei, Schlüssel {m.group(1)}"
                          + (" (Liste, Einträge mit ; trennen und abschließen)" if m.group(1) == "Keywords" else "")})
    return found


def sources(root):
    rs = sorted(glob.glob(os.path.join(root, "src", "**", "*.rs"), recursive=True))
    desk = sorted(glob.glob(os.path.join(root, "data", "*.desktop.in")))
    return rs, desk


def extract(root):
    rs, desk = sources(root)
    entries, index = [], {}
    for path in desk + rs:
        rel = os.path.relpath(path, root)
        new = extract_desktop(path, rel) if path.endswith(".desktop.in") else extract_rust(path, rel)
        for e in new:
            key = (e["ctxt"], e["id"])
            if key in index:
                old = index[key]
                old["refs"].append(e["ref"])
                if e.get("plural") != old.get("plural"):
                    raise SystemExit(f"{e['ref']}: dieselbe msgid mit anderer Pluralform")
                if e["note"] and e["note"] not in old["notes"]:
                    old["notes"].append(e["note"])
                continue
            e["refs"] = [e.pop("ref")]
            e["notes"] = [e.pop("note")] if e["note"] else []
            index[key] = e
            entries.append(e)
    return entries


# -------------------------------------------------------------------- po
def po_quote(s):
    s = s.replace("\\", "\\\\").replace('"', '\\"').replace("\t", "\\t").replace("\n", "\\n")
    return s


def po_field(key, s):
    if "\\n" in po_quote(s)[:-2] or len(s) > 70:
        chunks = re.split(r"(?<=\\n)", po_quote(s))
        return f'{key} ""\n' + "".join(f'"{c}"\n' for c in chunks if c)
    return f'{key} "{po_quote(s)}"\n'


def write_po(path, header, entries, obsolete=()):
    out = []
    for line in header["comments"]:
        out.append(line + "\n")
    out.append('msgid ""\n')
    out.append('msgstr ""\n')
    for k, v in header["fields"]:
        out.append(f'"{k}: {po_quote(v)}\\n"\n')
    for e in entries:
        out.append("\n")
        for n in e.get("notes", []):
            out.append(f"#. {n}\n")
        out.append("#: " + " ".join(e["refs"]) + "\n")
        flags = list(e.get("flags", []))
        if e.get("fuzzy"):
            flags.insert(0, "fuzzy")
        if flags:
            out.append("#, " + ", ".join(flags) + "\n")
        if e["ctxt"] is not None:
            out.append(po_field("msgctxt", e["ctxt"]))
        out.append(po_field("msgid", e["id"]))
        if e.get("plural") is not None:
            out.append(po_field("msgid_plural", e["plural"]))
            strs = e.get("strs") or ["", ""]
            for i, s in enumerate(strs):
                out.append(po_field(f"msgstr[{i}]", s))
        else:
            out.append(po_field("msgstr", e.get("str", "")))
    for e in obsolete:
        out.append("\n")
        for line in po_field("msgid", e["id"]).splitlines():
            out.append("#~ " + line + "\n")
        for line in po_field("msgstr", e.get("str", "") or "").splitlines():
            out.append("#~ " + line + "\n")
    with open(path, "w", encoding="utf-8") as f:
        f.write("".join(out))


def po_unquote(s):
    out, i = [], 0
    while i < len(s):
        c = s[i]
        if c == "\\":
            n = s[i + 1]
            out.append({"n": "\n", "t": "\t", '"': '"', "\\": "\\", "r": "\r"}[n])
            i += 2
        else:
            out.append(c)
            i += 1
    return "".join(out)


def read_po(path):
    """Liest eine .po/.pot; liefert (Kopf, Einträge). Veraltete (#~) werden übergangen."""
    header = {"comments": [], "fields": []}
    entries, cur, field = [], None, None

    def finish():
        if cur is not None:
            entries.append(cur)

    for raw in open(path, encoding="utf-8"):
        line = raw.rstrip("\n")
        if line.startswith("#~"):
            continue
        if not line.strip():
            finish()
            cur, field = None, None
            continue
        if cur is None:
            cur = {"ctxt": None, "refs": [], "notes": [], "flags": [], "comments": []}
        if line.startswith("#."):
            cur["notes"].append(line[2:].strip())
        elif line.startswith("#:"):
            cur["refs"].extend(line[2:].split())
        elif line.startswith("#,"):
            cur["flags"].extend(f.strip() for f in line[2:].split(","))
        elif line.startswith("#"):
            cur["comments"].append(line)
        else:
            m = re.match(r'^(msgctxt|msgid_plural|msgid|msgstr(?:\[(\d+)\])?)\s+"(.*)"$', line)
            if m:
                key = m.group(1)
                val = po_unquote(m.group(3))
                if key.startswith("msgstr["):
                    cur.setdefault("strs", [])
                    cur["strs"].append(val)
                    field = ("strs", len(cur["strs"]) - 1)
                else:
                    name = {"msgctxt": "ctxt", "msgid": "id", "msgid_plural": "plural", "msgstr": "str"}[key]
                    cur[name] = val
                    field = (name, None)
            elif line.startswith('"') and field:
                val = po_unquote(line[1:-1])
                if field[1] is None:
                    cur[field[0]] += val
                else:
                    cur["strs"][field[1]] += val
            else:
                raise SystemExit(f"{path}: unverständliche Zeile: {line}")
    finish()
    if entries and entries[0].get("id") == "" and entries[0]["ctxt"] is None:
        h = entries.pop(0)
        header["comments"] = h["comments"]
        for line in h.get("str", "").split("\n"):
            if ":" in line:
                k, v = line.split(":", 1)
                header["fields"].append((k, v.strip()))
    for e in entries:
        e["fuzzy"] = "fuzzy" in e["flags"]
        e["flags"] = [f for f in e["flags"] if f != "fuzzy"]
    return header, entries


def pot_header():
    return {
        "comments": [
            f"# Übersetzungsvorlage für {PACKAGE}.",
            "# Copyright (C) 2026 Querencia Desktop contributors",
            f"# Diese Datei steht unter derselben Lizenz wie {PACKAGE} (GPL-3.0-or-later).",
            "# Erzeugt mit werkzeug/i18n.py pot – nicht von Hand bearbeiten.",
            "#",
        ],
        "fields": [
            ("Project-Id-Version", DOMAIN),
            ("Report-Msgid-Bugs-To", ""),
            ("POT-Creation-Date", ""),
            ("PO-Revision-Date", "YEAR-MO-DA HO:MI+ZONE"),
            ("Last-Translator", "FULL NAME <EMAIL@ADDRESS>"),
            ("Language-Team", "LANGUAGE <LL@li.org>"),
            ("Language", ""),
            ("MIME-Version", "1.0"),
            ("Content-Type", "text/plain; charset=UTF-8"),
            ("Content-Transfer-Encoding", "8bit"),
            ("Plural-Forms", "nplurals=INTEGER; plural=EXPRESSION;"),
        ],
    }


def root_dir():
    return os.getcwd()


def cmd_pot(root):
    entries = extract(root)
    path = os.path.join(root, "po", DOMAIN + ".pot")
    write_po(path, pot_header(), entries)
    print(f"{os.path.relpath(path, root)}: {len(entries)} Texte")
    return entries


def linguas(root):
    with open(os.path.join(root, "po", "LINGUAS"), encoding="utf-8") as f:
        return [w for line in f for w in line.split("#", 1)[0].split()]


def cmd_update(root):
    _, pot = read_po(os.path.join(root, "po", DOMAIN + ".pot"))
    for lang in linguas(root):
        path = os.path.join(root, "po", lang + ".po")
        if os.path.exists(path):
            header, old = read_po(path)
        else:
            header = {"comments": [f"# {PACKAGE}: Übersetzung ({lang}).", "#"],
                      "fields": [(k, v) for k, v in pot_header()["fields"]]}
            old = []
        fields = dict(header["fields"])
        fields["Project-Id-Version"] = DOMAIN
        fields["Language"] = lang
        fields["Content-Type"] = "text/plain; charset=UTF-8"
        if lang in HEADER_PLURALS:
            fields.setdefault("Plural-Forms", HEADER_PLURALS[lang])
            if "INTEGER" in fields["Plural-Forms"]:
                fields["Plural-Forms"] = HEADER_PLURALS[lang]
        order = [k for k, _ in pot_header()["fields"]]
        header["fields"] = [(k, fields[k]) for k in order if k in fields] + \
            [(k, v) for k, v in fields.items() if k not in order]
        by_key = {(e["ctxt"], e["id"]): e for e in old}
        merged, used = [], set()
        for p in pot:
            e = dict(p)
            o = by_key.get((p["ctxt"], p["id"]))
            if o is not None:
                used.add((p["ctxt"], p["id"]))
                e["fuzzy"] = o.get("fuzzy", False)
                if p.get("plural") is not None:
                    e["strs"] = o.get("strs") or ["", ""]
                else:
                    e["str"] = o.get("str", "")
            merged.append(e)
        obsolete = [o for o in old if (o["ctxt"], o["id"]) not in used and o.get("str")]
        write_po(path, header, merged, obsolete)
        done = sum(1 for e in merged if translated(e))
        print(f"po/{lang}.po: {done}/{len(merged)} übersetzt, {len(obsolete)} veraltet")


def translated(e):
    if e.get("fuzzy"):
        return False
    if e.get("plural") is not None:
        return bool(e.get("strs")) and all(e["strs"])
    return bool(e.get("str"))


# -------------------------------------------------------------------- mo
def mo_bytes(header, entries):
    cat = {}
    hdr = "".join(f"{k}: {v}\n" for k, v in header["fields"])
    cat[b""] = hdr.encode()
    for e in entries:
        if not translated(e):
            continue
        key = e["id"]
        if e["ctxt"] is not None:
            key = e["ctxt"] + "\x04" + key
        if e.get("plural") is not None:
            k = (key + "\0" + e["plural"]).encode()
            v = "\0".join(e["strs"]).encode()
        else:
            k, v = key.encode(), e["str"].encode()
        cat[k] = v
    keys = sorted(cat)
    n = len(keys)
    ids = b"".join(k + b"\0" for k in keys)
    strs = b"".join(cat[k] + b"\0" for k in keys)
    o_ids, o_strs = 28, 28 + 8 * n
    base = 28 + 16 * n
    tab_ids, tab_strs, off = [], [], 0
    for k in keys:
        tab_ids.append((len(k), base + off))
        off += len(k) + 1
    base2 = base + len(ids)
    off = 0
    for k in keys:
        tab_strs.append((len(cat[k]), base2 + off))
        off += len(cat[k]) + 1
    out = struct.pack("<7I", 0x950412DE, 0, n, o_ids, o_strs, 0, base)
    out += b"".join(struct.pack("<2I", *t) for t in tab_ids)
    out += b"".join(struct.pack("<2I", *t) for t in tab_strs)
    return out + ids + strs


def cmd_mo(root, target):
    for lang in linguas(root):
        header, entries = read_po(os.path.join(root, "po", lang + ".po"))
        d = os.path.join(target, lang, "LC_MESSAGES")
        os.makedirs(d, exist_ok=True)
        path = os.path.join(d, DOMAIN + ".mo")
        data = mo_bytes(header, entries)
        with open(path, "wb") as f:
            f.write(data)
        print(f"{path}: {sum(1 for e in entries if translated(e))} Texte, {len(data)} Bytes")


# --------------------------------------------------------------- desktop
def cmd_desktop(root, src, dst):
    cats = {}
    for lang in linguas(root):
        _, entries = read_po(os.path.join(root, "po", lang + ".po"))
        cats[lang] = {e["id"]: e["str"] for e in entries
                      if e["ctxt"] is None and e.get("plural") is None and translated(e)}
    out = []
    for line in open(src, encoding="utf-8"):
        # Wie msgfmt --desktop (GNU gettext 0.23): übersetzte Zeilen VOR dem
        # Schlüssel, Sprachen alphabetisch – so erzeugen beide Werkzeuge
        # byte-gleiche Dateien.
        m = re.match(r"^(%s)=(.*)$" % "|".join(DESKTOP_KEYS), line.rstrip("\n"))
        if m:
            for lang in sorted(cats):
                # auch gleichlautende Übersetzungen (Name[de] gleich
                # Name), wie msgfmt --desktop
                t = cats[lang].get(m.group(2))
                if t:
                    out.append(f"{m.group(1)}[{lang}]={t}\n")
        out.append(line)
    with open(dst, "w", encoding="utf-8") as f:
        f.write("".join(out))
    print(f"{dst}: übersetzt aus {src}")


# ---------------------------------------------------------------- prüfen
PLACEHOLDER = re.compile(r"\{[a-z_]+\}")


def mnemonic(s):
    return len(re.findall(r"(?<!_)_(?=[^_\s])", s))


def cmd_pruefen(root):
    fehler = []
    path = os.path.join(root, "po", DOMAIN + ".pot")
    _, pot = read_po(path)
    fresh = extract(root)
    key = lambda e: (e["ctxt"], e["id"], e.get("plural"))
    if sorted(map(key, pot), key=str) != sorted(map(key, fresh), key=str):
        fehler.append("po/%s.pot ist nicht aktuell (werkzeug/i18n.py pot)" % DOMAIN)
    want = {(e["ctxt"], e["id"]) for e in fresh}
    for lang in linguas(root):
        _, entries = read_po(os.path.join(root, "po", lang + ".po"))
        have = {(e["ctxt"], e["id"]): e for e in entries}
        for k in sorted(want - set(have), key=str):
            fehler.append(f"{lang}: fehlt: {k[1]!r}")
        for k, e in have.items():
            if k not in want:
                fehler.append(f"{lang}: überzählig: {k[1]!r}")
                continue
            if not translated(e):
                fehler.append(f"{lang}: unübersetzt oder fuzzy: {e['id']!r}")
                continue
            ids = [e["id"]] + ([e["plural"]] if e.get("plural") is not None else [])
            outs = e["strs"] if e.get("plural") is not None else [e["str"]]
            want_ph = set(PLACEHOLDER.findall("".join(ids)))
            for s in outs:
                got = set(PLACEHOLDER.findall(s))
                # msgstr[0] darf {n} weglassen („Ein Vorkommen ersetzt.“)
                if got != want_ph and not (e.get("plural") is not None and got == want_ph - {"{n}"}):
                    fehler.append(f"{lang}: Platzhalter {sorted(got)} statt {sorted(want_ph)}: {e['id']!r}")
                if mnemonic(e["id"]) != mnemonic(s):
                    fehler.append(f"{lang}: Zugriffstaste (_) fehlt oder doppelt: {e['id']!r} -> {s!r}")
                if e["id"].endswith("\n") != s.endswith("\n"):
                    fehler.append(f"{lang}: Zeilenende abweichend: {e['id']!r}")
    import tempfile
    for src in sources(root)[1]:
        dst = src[: -len(".in")]
        with tempfile.TemporaryDirectory() as tmp:
            neu = os.path.join(tmp, "x.desktop")
            import contextlib, io
            with contextlib.redirect_stdout(io.StringIO()):
                cmd_desktop(root, src, neu)
            if not os.path.exists(dst) or open(neu, "rb").read() != open(dst, "rb").read():
                fehler.append(f"{os.path.relpath(dst, root)} ist nicht aktuell (werkzeug/i18n.py desktop)")
    for f in fehler:
        print("FEHLER", f)
    print(f"pruefen: {len(fresh)} Texte, Sprachen {' '.join(linguas(root))}: "
          + ("PASS" if not fehler else f"FAIL ({len(fehler)})"))
    return 1 if fehler else 0


def main(argv):
    root = root_dir()
    if len(argv) < 2:
        print(__doc__)
        return 2
    cmd = argv[1]
    if cmd == "pot":
        cmd_pot(root)
    elif cmd == "update":
        cmd_update(root)
    elif cmd == "mo" and len(argv) == 3:
        cmd_mo(root, argv[2])
    elif cmd == "desktop" and len(argv) == 4:
        cmd_desktop(root, argv[2], argv[3])
    elif cmd == "pruefen":
        return cmd_pruefen(root)
    else:
        print(__doc__)
        return 2
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
