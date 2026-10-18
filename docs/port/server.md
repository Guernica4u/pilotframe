# The server side (server scripts)

Owner's decision (2026-10-17): our game plays the server's part by running the game's **own server scripts** in a second VM (`crates/pf-servervm`). The client scripts depend on state only the server sends: the game state (`gameState` and the other server vars), entity vars, the player's saved data (level, XP), and remote calls.

## What the install has

- **The scripts:** the server's scripts are in the same VPK as the client's: `vscript_server.nut`, `_mapspawn.nut` and what they include (76 files by the literal includes; game modes and map scripts are included by name at run time).
- **No server code:** the PC game ships no `server.dll` (`bin/x64_retail` has `dedicated.dll`, `AdminServer.dll` and the engine, but no game server). So the server's engine functions can't be read from code. Each one is:
  - **ported from its client.dll twin** where client.dll registers a function of the same name and purpose (time, random numbers, includes, game rules, playlists, persistence enums…); or
  - **worked out from the scripts** (how they call it, what they expect back), marked "ours" with what it's based on.
- **Count** (`script_calls.py`, server set): 341 functions called that no script defines; 163 of them have a name in client.dll's inventory, 178 don't. Entity methods come on top.

## Startup (ours)

Mirrors client.dll's order (docs/port/scripts.md), since the server's own order can't be read: `init.nut`, `_consts.nut`, `_threads.nut`; the server functions; `vscript_server.nut`; `_mapspawn` included into the root table. `IsServer()` is true, `IsClient()`/`IsUI()` false.

## Remote calls (server → client)

- Both VMs run `_remote_functions.nut`, which registers the same list of client functions in the same order; a function's number is its place in that list (the registration is client.dll's `CRemoteFunctions`, ported in pf-clientvm).
- The server's `Remote.CallFunction_Replay`, `CallFunction_NonReplay` and `CallFunction_UI( player, name, ... )` queue a call for that player.
- **What a call carries** (from client.dll's reader, 0x180427010): an 8-bit function number, a flag, a 4-bit argument count (more than 10 is refused), then per argument a 2-bit type: null, bool (1 bit), 32-bit integer, 32-bit float. So entities travel as numbers (scripts pass `GetEncodedEHandle()`), and floats arrive exactly. A client keeps at most 600 queued calls.
- **Running them** (0x180427850): each frame, for the local player, each call's function is found by number, looked up through the player's script scope and called with that scope as `this` and the arguments; a UI call runs in the UI VM instead (we don't run the UI VM yet: those are skipped, placeholder).
- **Ours:** the server-side checks (a name that isn't registered, an argument of another type, more than 10) raise errors with our own wording, since the server's checks aren't in the install. In our game the calls go straight from the server VM's outbox to the client VM (`ClientVm::run_remote_calls`); there is no network message.

## Survey

`cargo run -p pf-servervm --example survey [map] [frames]` runs the server startup from the install and lists every missing function and method, marking those with a client.dll twin. Current state: `_mapspawn.nut` runs until it creates its first entity (`Entities.CreateByClassname`, `CreateEntity`, `DispatchSpawn`): the server's entity system is next.

## Next

1. The server's entity system: `Entities` (`CreateByClassname`, `FindByClassname`…), `CreateEntity`, `DispatchSpawn`, the server script classes (`CBaseEntity`, `CBaseAnimating`, `CBaseCombatCharacter`, `CBasePlayer`, `CAI_BaseNPC`, `CTitanSoul`, `CHardPointEntity`, `CWeaponX`…) and their class files (`class/cbaseentity.nut` and the rest). Shared with the game (pf-game) through the host.
2. Players joining (`CodeCallback_OnClientConnected` and friends), server vars and entity vars reaching the client.
3. Game mode and map scripts.
