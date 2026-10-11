# The game's scripts (Squirrel)

What we know about how Titanfall runs its Squirrel scripts, from the scripts in the EA install and the DLLs (`launcher.dll`, `engine.dll`, `client.dll`). Our interpreter is `crates/pf-squirrel`.

## Where things are

- **The scripts:** 481 `.nut` files (7.3 MB) under `scripts/vscripts/` in `englishclient_mp_common.bsp.pak000_dir.vpk`, plus 75 `.menu` and the `.res` layouts.
- **The interpreter:** **Squirrel 2.2.3 stable** (its version string), built into `launcher.dll`. `dedicated.dll` carries a copy.
- **No server code:** the PC game ships no `server.dll` (multiplayer ran on Respawn's servers), so the server VM's native functions aren't in the install. Where server scripts need one, its behaviour has to be worked out from the scripts and the client's matching code.

## The script VMs and how they start

- `launcher.dll` creates each VM and runs `scripts/vscripts/init.nut`, `_consts.nut` and `_threads.nut` in it.
- `client.dll` runs `vscript_client.nut` in the client VM and `vscript_ui.nut` in the UI VM. The client VM's map scripts start from `Cl_MapspawnMain` in `_cl_mapspawn.nut`.
- Scripts pull in others with `IncludeFile`, `IncludeScript`, `IncludeFileAllowMultipleLoads` and `DoIncludeScript`. Following the literal includes:
  - the **client VM** loads 168 files;
  - the **UI VM** loads 83.
  - A few includes name files that aren't in the install (server-only and dev scripts such as `mp/_ranked`, `test/cl_myscript`).
- **Code calls into scripts:** about 97 `CodeCallback_*` functions (level init and shutdown, HUD think, entity creation and destruction, death, crosshair target and so on). The engine calls these by name when the event happens.

## The language as the game uses it

Squirrel 2.2 (its public reference manual), with these additions and quirks, all checked against every script:

- **Thread keywords** (Respawn): `thread f()`, `waitthread f()`, `waitthreadsolo f()`, `delaythread( t ) f()`, and `wait t`. They map onto the native thread functions (below).
- **Default parameter values:** `function f( a, b = null )`. This is a Squirrel 3 feature, added to 2.2.3.
- **Free variables** (Squirrel 2): `function() : ( a, b ) { ... }` copies outer values into the function when it's created. Used for callbacks such as `OnThreadEnd`.
- **`delegate parent : table`** (Squirrel 2): sets a table's delegate.
- **`base` is not a keyword:** scripts use it as a variable name.
- **Number reading:** like Squirrel's reader, a number keeps taking digits and dots, and only the part before a second dot counts. One script has `-100.0.0`, read as -100.0.
- **Not used anywhere:** generators (`yield`/`resume`), `@` lambdas, verbatim strings, `<=>`, `>>>`, `static`, `extends`, and a preprocessor.
- **Used** (counts across all scripts):
  - `thread` 1,492, `wait` 1,300, `waitthread` 182, `delaythread` 122;
  - `<-` 8,053, `const` 2,479, `foreach` 1,635, `switch` 311, `enum` 87;
  - `class` 16 (with `constructor`), `vargv`/`vargc` 131, `::` root access 448, `delete` 116, `clone` 53, `typeof` 43, `instanceof` 25, `try` 4, `throw` 2.
  - Metamethods: `_get`, `_set`, `_newslot`, `_nexti`, `_tostring`, `_typeof`.
  - Built-in methods: `.weakref()`, `.bindenv()`, `.call()`, `.acall()`, `.getinfos()`.

**Parser check:** all 481 scripts parse (`crates/pf-squirrel/tests/game_scripts.rs`, which runs when the install is present).

## Native functions (what scripts can call in the engine)

### How they're registered

- **client.dll and engine.dll** use one 0x78-byte record per function:

  | Offset | Field |
  |---|---|
  | +0x00 | script name |
  | +0x08 | C++ name |
  | +0x10 | help text |
  | +0x18 | type-mask string |
  | +0x20 | parameter count (−2 = variable) |
  | +0x38 | return type |
  | +0x40 | parameter types, a list of ints |
  | +0x60 | call thunk |
  | +0x68 | the function |

  Global functions sit at fixed addresses. Class methods go into a per-class array (for example `CClientScriptHudElement`, "Hud Element Control").
- **Type codes:** 0 void, 1 float, 3 vector, 5 int, 6 bool, 32 string, 33 a script object, 0x1f variant. The test function `NativeFuncTest` ("takes int, bool, float, returns 1") confirms 5, 6 and 1.
- **launcher.dll** registers Squirrel's standard library and its own functions with Squirrel tables:
  - 32-byte `{name, function, parameter check, type mask}` entries: the base library, the methods for each value type, and the `Vector` class (`Length`, `Dot`, `Cross`, `AnglesToForward`…);
  - 16-byte `{name, function}` pairs for the thread system: `SpinOff`, `SpinOffDelayed`, `SpinOffAndWaitFor`, `SpinOffAndWaitForSolo`, `Wait`, `WaitEndFrame`, `RegisterSignal`, `IsValidSignal`, `Signal`, `WaitSignal`, `WaitSignalOnDeadEnt`, `WaitSignalTimeout`, `EndSignal`, `OnThreadEnd`;
  - a few direct registrations (`developer`, `GetFunctionSignature`, `IsValid`).
- **Count:** 1,615 natives found across the three DLLs.

### What's still unaccounted for

These are called by scripts the VM loads, defined by no script, and not found in the DLLs yet:
- **Client VM:** 119. Most are server functions behind `IsServer()` checks in shared scripts, which never run on the client. The rest include `Assert`, the `Vector` constructor (registered as a class, not a function), `regexp` (Squirrel's string library), and a few dev helpers.
- **UI VM:** 20.

### Tools

The inventory and these checks are local research tools (docs/tools.md):
- `natives.py`: the inventory;
- `sqreg.py`: the static tables;
- `script_calls.py`: calls against the inventory, per VM;
- `include_graph.py`: the load order;
- `lang_survey.py`: the feature counts.
