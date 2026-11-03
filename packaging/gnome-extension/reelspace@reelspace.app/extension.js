// Reelspace companion extension (GNOME 49-51).
// 1. Turns org.reelspace.Recorder1 signals into Shell OSD toasts. GNOME hides normal
//    notification banners while a fullscreen window is focused, but the OSD (the volume
//    popup) is drawn above fullscreen games, so it is the only in-game visual channel.
// 2. Serves org.reelspace.Shell so the daemon can learn the focused window, which apps
//    cannot query themselves on Wayland.
import Gio from 'gi://Gio';
import GLib from 'gi://GLib';
import Shell from 'gi://Shell';
import * as Main from 'resource:///org/gnome/shell/ui/main.js';
import {Extension} from 'resource:///org/gnome/shell/extensions/extension.js';

import {osdFor} from './osd.js';

const REC_NAME = 'org.reelspace.Recorder';
const REC_PATH = '/org/reelspace/Recorder';
const REC_IFACE = 'org.reelspace.Recorder1';

const SHELL_NAME = 'org.reelspace.Shell';
const SHELL_PATH = '/org/reelspace/Shell';
const SHELL_XML = `<node>
  <interface name="org.reelspace.Shell">
    <method name="FocusedWindow">
      <arg type="s" direction="out" name="title"/>
      <arg type="s" direction="out" name="app_id"/>
    </method>
    <signal name="FocusedWindowChanged">
      <arg type="s" name="title"/>
      <arg type="s" name="app_id"/>
    </signal>
  </interface>
</node>`;

const FOCUS_DEBOUNCE_MS = 250; // browser/editor titles change on every keystroke or tab switch

export default class ReelspaceExtension extends Extension {
    enable() {
        this._lastState = null;
        this._recorderSeen = false;

        this._signalSub = Gio.DBus.session.signal_subscribe(
            REC_NAME, REC_IFACE, null, REC_PATH, null, Gio.DBusSignalFlags.NONE,
            (_conn, _sender, _path, _iface, signal, params) =>
                this._onRecorderSignal(signal, params.deepUnpack()));

        // The daemon disappearing without a "stopped" state means it crashed or was killed.
        this._recWatch = Gio.bus_watch_name(Gio.BusType.SESSION, REC_NAME,
            Gio.BusNameWatcherFlags.NONE,
            () => { this._recorderSeen = true; },
            () => {
                if (this._recorderSeen && this._lastState !== 'stopped')
                    this._osd('media-playback-stop-symbolic', 'Recorder stopped');
                this._recorderSeen = false;
                this._lastState = null;
            });

        this._focusInfo = this._readFocus();
        this._dbus = Gio.DBusExportedObject.wrapJSObject(SHELL_XML, {
            FocusedWindow: () => this._focusInfo,
        });
        this._dbus.export(Gio.DBus.session, SHELL_PATH);
        this._nameId = Gio.bus_own_name_on_connection(Gio.DBus.session, SHELL_NAME,
            Gio.BusNameOwnerFlags.NONE, null, null);

        this._titleWindow = null;
        this._titleId = 0;
        this._focusTimer = 0;
        this._focusId = global.display.connect('notify::focus-window', () => {
            this._trackTitle(global.display.focus_window);
            this._queueFocusUpdate();
        });
        this._trackTitle(global.display.focus_window);
    }

    disable() {
        Gio.DBus.session.signal_unsubscribe(this._signalSub);
        Gio.bus_unwatch_name(this._recWatch);
        Gio.bus_unown_name(this._nameId);
        this._dbus.unexport();
        this._dbus = null;
        global.display.disconnect(this._focusId);
        this._trackTitle(null);
        if (this._focusTimer)
            GLib.source_remove(this._focusTimer);
        this._focusTimer = 0;
        this._focusInfo = null;
    }

    _onRecorderSignal(signal, args) {
        const osd = osdFor(signal, args, this._lastState);
        if (signal === 'StateChanged')
            this._lastState = args[0];
        if (osd)
            this._osd(...osd);
    }

    _osd(iconName, label) {
        Main.osdWindowManager.showAll(Gio.ThemedIcon.new(iconName), label);
    }

    // app_id: Flatpak/Snap id, else the .desktop id (without suffix), else WM_CLASS
    // (Proton games report e.g. "steam_app_2139460").
    _readFocus() {
        const w = global.display.focus_window;
        if (!w)
            return ['', ''];
        let appId = w.get_sandboxed_app_id() || '';
        if (!appId) {
            const app = Shell.WindowTracker.get_default().get_window_app(w);
            if (app && !app.is_window_backed())
                appId = (app.get_id() || '').replace(/\.desktop$/, '');
        }
        return [w.get_title() || '', appId || w.get_wm_class() || ''];
    }

    _trackTitle(win) {
        if (this._titleWindow && this._titleId) {
            try {
                this._titleWindow.disconnect(this._titleId);
            } catch {
                // window already finalized; its handlers went with it
            }
        }
        this._titleWindow = win;
        this._titleId = win ? win.connect('notify::title', () => this._queueFocusUpdate()) : 0;
    }

    _queueFocusUpdate() {
        if (this._focusTimer)
            return;
        this._focusTimer = GLib.timeout_add(GLib.PRIORITY_DEFAULT, FOCUS_DEBOUNCE_MS, () => {
            this._focusTimer = 0;
            const next = this._readFocus();
            if (next[0] !== this._focusInfo[0] || next[1] !== this._focusInfo[1]) {
                this._focusInfo = next;
                this._dbus.emit_signal('FocusedWindowChanged', new GLib.Variant('(ss)', next));
            }
            return GLib.SOURCE_REMOVE;
        });
    }
}
