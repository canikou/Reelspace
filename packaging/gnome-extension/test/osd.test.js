// Run: gjs -m packaging/gnome-extension/test/osd.test.js
import {formatDuration, osdFor} from '../reelspace@reelspace.app/osd.js';

const eq = (a, b, msg) => {
    if (JSON.stringify(a) !== JSON.stringify(b))
        throw new Error(`${msg}: got ${JSON.stringify(a)}, want ${JSON.stringify(b)}`);
};
eq(formatDuration(75), '1:15', 'm:ss');
eq(formatDuration(3725), '1:02:05', 'h:mm:ss');
eq(formatDuration(-3), '0:00', 'negative clamps');
eq(osdFor('ClipSaved', ['/c.mp4', 300.4, 'Once Human'], null), ['emblem-ok-symbolic', 'Clip saved 5:00 · Once Human'], 'clip saved with game');
eq(osdFor('ClipSaved', ['/c.mp4', 59, ''], null)[1], 'Clip saved 0:59', 'clip saved no game');
eq(osdFor('SessionStarted', ['Once Human', '/s.mkv'], null)[1], 'Recording Once Human', 'session start');
eq(osdFor('SessionEnded', ['Once Human', '/s.mkv', 5400], null)[1], 'Session saved 1:30:00 · Once Human', 'session end');
eq(osdFor('StateChanged', ['error', 'disk floor reached'], 'recording')[1], 'Recorder error: disk floor reached', 'error');
eq(osdFor('StateChanged', ['error', 'disk floor reached'], 'error'), null, 'repeated state deduped');
eq(osdFor('StateChanged', ['recording', ''], 'paused'), null, 'normal states silent');
eq(osdFor('ConfigChanged', [], null), null, 'unknown signals silent');
print('osd tests passed');
