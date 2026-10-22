#!/usr/bin/env python3
"""Querencia Screenshot 0.2.10 (aus Terminal 0.3.1, nach Systemmonitor 0.1.3/Bilder 0.2.4): Ersatz für
das Settings-Portal im isolierten Lauf.

Belegt auf dem eigenen Bus des Laufs den Namen org.freedesktop.portal.Desktop
und beantwortet org.freedesktop.portal.Settings (Version 2) so, wie es
xdg-desktop-portal-gtk live tut: org.freedesktop.appearance color-scheme
(0 = keine Vorliebe, 1 = dunkel) und alle Schlüssel von
org.gnome.desktop.interface. Anders als die Vorlage kommen **alle** Werte aus
den gsettings des isolierten Profils (dconf unter $XDG_CONFIG_HOME): hell und
dunkel schaltet der Lauf mit
  gsettings set org.gnome.desktop.interface color-scheme prefer-dark|default
und der Ersatz meldet jede Änderung als SettingChanged wie das echte Portal.

Aufruf (nur IN der isolierten Sitzung):  portal-ersatz.py <bereit-datei>
"""

import sys

from gi.repository import Gio, GLib

PFAD = "/org/freedesktop/portal/desktop"
XML = """
<node>
  <interface name="org.freedesktop.portal.Settings">
    <method name="ReadAll">
      <arg type="as" name="namespaces" direction="in"/>
      <arg type="a{sa{sv}}" name="value" direction="out"/>
    </method>
    <method name="Read">
      <arg type="s" name="namespace" direction="in"/>
      <arg type="s" name="key" direction="in"/>
      <arg type="v" name="value" direction="out"/>
    </method>
    <method name="ReadOne">
      <arg type="s" name="namespace" direction="in"/>
      <arg type="s" name="key" direction="in"/>
      <arg type="v" name="value" direction="out"/>
    </method>
    <signal name="SettingChanged">
      <arg type="s" name="namespace"/>
      <arg type="s" name="key"/>
      <arg type="v" name="value"/>
    </signal>
    <property name="version" type="u" access="read"/>
  </interface>
</node>
"""

bereit = sys.argv[1]
verbindung = None
einstellungen = Gio.Settings.new("org.gnome.desktop.interface")


def farbschema() -> int:
    return 1 if einstellungen.get_string("color-scheme") == "prefer-dark" else 0


def gnome_interface() -> dict:
    s = einstellungen
    return {k: s.get_value(k) for k in s.props.settings_schema.list_keys()}


def alles() -> dict:
    return {
        "org.freedesktop.appearance": {
            "color-scheme": GLib.Variant("u", farbschema()),
            "contrast": GLib.Variant("u", 0),
        },
        "org.gnome.desktop.interface": gnome_interface(),
    }


def passt(ns: str, muster: list) -> bool:
    if not muster:
        return True
    for m in muster:
        if m == ns or (m.endswith(".*") and ns.startswith(m[:-1])) or m == "":
            return True
    return False


def methode(con, sender, pfad, schnitt, name, args, aufruf):
    daten = alles()
    if name == "ReadAll":
        muster = args.unpack()[0]
        aus = {ns: werte for ns, werte in daten.items() if passt(ns, muster)}
        aufruf.return_value(GLib.Variant("(a{sa{sv}})", (aus,)))
        return
    ns, key = args.unpack()
    wert = daten.get(ns, {}).get(key)
    if wert is None:
        aufruf.return_dbus_error("org.freedesktop.portal.Error.NotFound", f"{ns} {key}")
        return
    if name == "Read":
        aufruf.return_value(GLib.Variant("(v)", (GLib.Variant("v", wert),)))
    else:
        aufruf.return_value(GLib.Variant("(v)", (wert,)))


def eigenschaft(con, sender, pfad, schnitt, name):
    return GLib.Variant("u", 2)


def bus_da(con, name):
    global verbindung
    verbindung = con
    info = Gio.DBusNodeInfo.new_for_xml(XML)
    for schnitt in info.interfaces:
        con.register_object(PFAD, schnitt, methode, eigenschaft, None)


def geaendert(s, key):
    if verbindung is None:
        return
    werte = [("org.gnome.desktop.interface", key, s.get_value(key))]
    if key == "color-scheme":
        werte.insert(0, ("org.freedesktop.appearance", "color-scheme", GLib.Variant("u", farbschema())))
    for ns, k, wert in werte:
        verbindung.emit_signal(None, PFAD, "org.freedesktop.portal.Settings", "SettingChanged",
                               GLib.Variant("(ssv)", (ns, k, wert)))
    print(f"portal-ersatz: {key} = {s.get_value(key)} gemeldet", flush=True)


def name_da(con, name):
    with open(bereit, "w") as f:
        f.write(f"{name} color-scheme {farbschema()}\n")
    print(f"portal-ersatz: {name} belegt, color-scheme {farbschema()}", flush=True)


def name_weg(con, name):
    print(f"portal-ersatz: Name {name} nicht erhalten", flush=True)
    schleife.quit()


einstellungen.connect("changed", geaendert)
# Ohne einen ersten Lesezugriff meldet GSettings keine Änderungen.
gnome_interface()
schleife = GLib.MainLoop()
Gio.bus_own_name(Gio.BusType.SESSION, "org.freedesktop.portal.Desktop",
                 Gio.BusNameOwnerFlags.DO_NOT_QUEUE, bus_da, name_da, name_weg)
schleife.run()
