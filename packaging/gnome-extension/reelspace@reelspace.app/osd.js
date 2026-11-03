// Pure signal -> OSD mapping, kept free of Shell imports so it can be tested with plain gjs.

// 75 -> "1:15", 3725 -> "1:02:05"
export function formatDuration(seconds) {
    const t = Math.max(0, Math.round(Number(seconds) || 0));
    const h = Math.floor(t / 3600), m = Math.floor((t % 3600) / 60), s = t % 60;
    const ss = String(s).padStart(2, '0');
    return h > 0 ? `${h}:${String(m).padStart(2, '0')}:${ss}` : `${m}:${ss}`;
}

// Maps a recorder signal to [icon, label], or null for signals that show nothing.
// lastState dedupes StateChanged so a backoff loop cannot spam the screen.
export function osdFor(signal, args, lastState) {
    const withGame = (text, game) => (game ? `${text} · ${game}` : text);
    switch (signal) {
    case 'ClipSaving':
        return ['media-record-symbolic', 'Saving clip…'];
    case 'ClipSaved':
        return ['emblem-ok-symbolic', withGame(`Clip saved ${formatDuration(args[1])}`, args[2])];
    case 'ClipFailed':
        return ['dialog-warning-symbolic', 'Clip failed'];
    case 'SessionStarted':
        return ['media-record-symbolic', `Recording ${args[0] || 'session'}`];
    case 'SessionEnded':
        return ['emblem-ok-symbolic', withGame(`Session saved ${formatDuration(args[2])}`, args[0])];
    case 'StateChanged': {
        const [state, detail] = args;
        if (state === lastState)
            return null;
        if (state === 'error')
            return ['dialog-warning-symbolic', detail ? `Recorder error: ${detail}` : 'Recorder error'];
        if (state === 'stopped')
            return ['media-playback-stop-symbolic', 'Recorder stopped'];
        return null;
    }
    default:
        return null;
    }
}
