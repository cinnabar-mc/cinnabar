// Generic native terrain observations. Only explicit load queries may create temporary areas.
import { world, system, ScriptEventSource } from '@minecraft/server';
import config from './config.js';

const subscriptions = new Map();
const pendingAreas = new Set();
const ownedAreas = new Set();
const retiredAreas = new Set();
let tick = 0;
let cursor = 0;
const ascii = text => text.replace(/[\u007f-\uffff]/g, c => '\\u' + c.charCodeAt(0).toString(16).padStart(4, '0'));
function report(sub, region) {
    const data = ascii(JSON.stringify({error: region === undefined, region: region ?? null}));
    if (data.length > config.maxResponseBytes) return;
    const total = Math.ceil(data.length / config.partBytes);
    const sequence = ++sub.sequence;
    for (let part = 0; part < total; ++part) {
        console.warn(config.prefix + JSON.stringify({version: config.version, session: config.session,
            nonce: config.nonce, id: sub.id, sequence, part, total,
            data: data.slice(part * config.partBytes, (part + 1) * config.partBytes)}));
    }
}

function release(sub) {
    sub.closed = true;
    // A pending create also removes itself on completion when this subscription was retired.
    if (sub.area) {
        retiredAreas.add(sub.area);
        removeArea(sub.area);
        sub.area = undefined;
    }
    return sub.pending ?? Promise.resolve();
}

function removeArea(id) {
    try {
        const manager = world.tickingAreaManager;
        if (manager.hasTickingArea(id)) manager.removeTickingArea(id);
        if (!manager.hasTickingArea(id)) { ownedAreas.delete(id); retiredAreas.delete(id); }
    } catch (_) { /* Keep the owned slot reserved and retry cleanup next tick. */ }
}

function valid(query) {
    return query && Number.isInteger(query.dimension) && query.dimension >= 0 && query.dimension < config.dimensions.length
        && typeof query.load === 'boolean' && Array.isArray(query.min) && query.min.length === 3
        && Array.isArray(query.size) && query.size.length === 3
        && query.min.every(n => Number.isInteger(n) && Math.abs(n) <= config.maxCoordinate)
        && query.size.every(n => Number.isInteger(n) && n > 0 && n <= config.maxExtent)
        && query.min.every((n, i) => n + query.size[i] <= 2147483647);
}

function create(request) {
    const sub = {id: request.id, query: request.query, touched: tick, sequence: 0,
        closed: false, loading: false, failed: false, scan: undefined, area: undefined};
    sub.retry = () => {
        sub.lastRetry = tick;
        sub.failed = false;
        try {
            sub.dimension = world.getDimension(config.dimensions[sub.query.dimension]);
            if (sub.query.load) {
                sub.loading = true;
                const [x, y, z] = sub.query.min;
                const [sx, sy, sz] = sub.query.size;
                const area = 'cxb_terrain_' + config.nonce + '_' + sub.id;
                const manager = world.tickingAreaManager;
                if (!manager) throw new Error('TickingAreaManager unavailable');
                if (pendingAreas.size + ownedAreas.size >= config.maxQueries) throw new Error('Area budget exhausted');
                sub.pending = manager.createTickingArea(area, {dimension: sub.dimension, from: {x, y, z},
                    to: {x: x + sx - 1, y: y + sy - 1, z: z + sz - 1}}).then(() => {
                    ownedAreas.add(area);
                    if (sub.closed) { retiredAreas.add(area); removeArea(area); }
                    else sub.area = area;
                    sub.loading = false;
                }).catch(() => { sub.loading = false; sub.failed = true; });
                pendingAreas.add(sub.pending);
                sub.pending.then(() => pendingAreas.delete(sub.pending));
            }
        } catch (_) { sub.loading = false; sub.failed = true; }
    };
    sub.retry();
    return sub;
}

system.afterEvents.scriptEventReceive.subscribe(event => {
    if (event.id !== config.event || event.sourceType !== ScriptEventSource.Server
        || event.message.length > config.maxQueries * 512 + 512) return;
    let message;
    try { message = JSON.parse(event.message); } catch (_) { return; }
    if (message.version !== config.version || message.session !== config.session
        || message.nonce !== config.nonce || !Array.isArray(message.queries)
        || message.queries.length > config.maxQueries) return;
    const ids = new Set();
    for (const request of message.queries) {
        if (!request || !Number.isInteger(request.id) || request.id < 1 || request.id > 0xffffffff
            || ids.has(request.id) || !valid(request.query)) return;
        ids.add(request.id);
    }
    const released = [];
    for (const [id, sub] of subscriptions) {
        if (!ids.has(id)) { released.push(release(sub)); subscriptions.delete(id); }
    }
    for (const request of message.queries) {
        const previous = subscriptions.get(request.id);
        if (previous && JSON.stringify(previous.query) !== JSON.stringify(request.query)) {
            release(previous);
            subscriptions.delete(request.id);
        }
        const sub = subscriptions.get(request.id) ?? create(request);
        sub.touched = tick;
        subscriptions.set(request.id, sub);
    }
    if (ids.size === 0 && message.stop === true) Promise.all([...released, ...pendingAreas]).then(() => {
        if (ownedAreas.size !== 0) return;
        console.warn(config.prefix + JSON.stringify({version: config.version, session: config.session,
            nonce: config.nonce, id: 0, sequence: 0, part: 0, total: 1, data: 'stop'}));
    });
});

function sample(sub) {
    if (sub.loading || sub.closed || sub.sampledTick === tick) return;
    if (sub.failed) {
        sub.sampledTick = tick;
        if (tick % config.retryTicks === 0) report(sub, undefined);
        return;
    }
    const query = sub.query;
    const [sx, sy, sz] = query.size;
    const count = sx * sy * sz;
    if (count > config.maxCells) return;
    const scan = sub.scan ?? {index: 0, palette: [], cells: [], keys: new Map(), bytes: 0};
    sub.scan = scan;
    const i = scan.index++;
    // Nested x, z, y order: index = (x * sizeZ + z) * sizeY + y.
    const x = query.min[0] + Math.floor(i / (sz * sy));
    const z = query.min[2] + Math.floor(i / sy) % sz;
    const y = query.min[1] + i % sy;
    let cell = config.unknown;
    try {
        const block = sub.dimension.getBlock({x, y, z});
        if (block) {
            const raw = block.permutation.getAllStates();
            const canonical = {};
            for (const key of Object.keys(raw).sort()) canonical[key] = raw[key];
            const state = {name: block.typeId, states: JSON.stringify(canonical)};
            const key = JSON.stringify(state);
            if (scan.keys.has(key)) cell = scan.keys.get(key);
            else {
                // Escaped ASCII length bounds UTF-8 metadata, including non-ASCII state strings.
                const bytes = ascii(state.name).length + ascii(state.states).length;
                if (state.name.length <= config.maxNameBytes && ascii(state.states).length <= config.maxStateBytes
                    && scan.bytes + bytes <= config.maxMetadataBytes && scan.palette.length < config.maxCells) {
                    cell = scan.palette.length;
                    scan.keys.set(key, cell);
                    scan.palette.push(state);
                    scan.bytes += bytes;
                }
            }
        }
    } catch (_) { /* Missing or out-of-height blocks remain UNKNOWN, never synthetic air. */ }
    scan.cells.push(cell);
    if (scan.index === count) {
        sub.sampledTick = tick;
        report(sub, {session: Number(config.session), revision: 0, palette: scan.palette, cells: scan.cells});
        sub.scan = undefined;
    }
}

system.runInterval(() => {
    ++tick;
    for (const id of retiredAreas) removeArea(id);
    for (const [id, sub] of subscriptions) {
        if (tick - sub.touched >= config.ttlTicks) { release(sub); subscriptions.delete(id); }
        else if (sub.failed && !sub.loading && tick - sub.lastRetry >= config.retryTicks) sub.retry();
    }
    const active = [...subscriptions.values()];
    if (active.length === 0) return;
    // One global work budget, shared fairly across every retained query.
    for (let i = 0; i < config.cellsPerTick; ++i) {
        cursor = (cursor + 1) % active.length;
        sample(active[cursor]);
    }
}, 1);
