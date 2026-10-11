# The game's scripts (Squirrel)

What we know about how Titanfall runs its Squirrel scripts, from the scripts in the EA install and the DLLs (`launcher.dll`, `engine.dll`, `client.dll`). Our interpreter is `crates/pf-squirrel`.

## Where things are

- **The scripts:** 481 `.nut` files (7.3 MB) under `scripts/vscripts/` in `englishclient_mp_common.bsp.pak000_dir.vpk`, plus 75 `.menu` and the `.res` layouts.
- **The interpreter:** **Squirrel 2.2.3 stable** (its version string), built into `launcher.dll`. `dedicated.dll` carries a copy.
- **No server code:** the PC game ships no `server.dll` (multiplayer ran on Respawn's servers), so the server VM's native functions aren't in the install. Where server scripts need one, its behaviour has to be worked out from the scripts and the client's matching code.

## R1Delta as a reference

- **Its interpreter is the original:** R1Delta's `launcher.dll` is byte-identical to the EA install's (same SHA-256), so script behaviour measured in R1Delta (the oracle, below) is the real game's.
- **Its scripts are not:** R1Delta ships its own `scripts/vscripts/`, in its install folder under `r1delta/`. Of its 478 files:
  - 47 match the originals;
  - 131 are changed, including the HUD's own (`cl_main_hud.nut`, `_cl_misc_hud.nut`, `cl_titan_hud.nut`) and many weapons;
  - 300 are new.
- **What that means for reference shots:** R1Delta's HUD is not the original HUD. Its changes sit behind `delta_hud_*` console variables (level-up banner, XP bar, chat, Titan earnings, anti-Titan hint and so on). Reference shots need those set to the original behaviour. Even then, the code around them differs, so for the HUD the original scripts, run by our interpreter, are the reference.
- **Running a test script in R1Delta:**
  - R1Delta adds a `script_client` console command.
  - Its loose `scripts/vscripts/` folder is searched for scripts, so a file placed there can be run with `script_client IncludeScript( "name" )` while in a map.

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

**Measured in the real game** (the script oracle, `crates/pf-squirrel/oracle/`; run 1 results in `game_results_run1.txt`):

- **Grammar:**
  - An exponent needs a decimal point: `1e3` is "invalid numeric format", `1.0e3` is fine.
  - There's no `<=>`: "expression expected".
  - `@` only starts a verbatim string (`@"..."`); `@(...)` is "string expected".
  - Default parameters can't be combined with `...`: "function with default parameters cannot have variable number of parameters".
  - **A statement ending in `}` takes no `;`.** A `;` after it is an empty statement of its own, which then needs `;` or a line break, so `local t = {}; f()` on one line fails with "end of statement expected (; or lf)".
- **Integers are 32-bit:**
  - `2147483647 + 1` = −2147483648, and `3000000000` reads as −1294967296.
  - Shifts use the low 5 bits of the count (`1 << 32` = 1, `1 << 33` = 2); `>>>` exists.
  - Division truncates (`-7 / 2` = −3), and `%` takes the dividend's sign (`7 % -3` = 1, `-7 % 3` = −1).
  - Integer `1 / 0` throws "division by zero"; `10 / 0.0` = `1.#INF`.
- **Floats are 32-bit:**
  - `16777217.0` prints `1.67772e+007`.
  - Printed like C's `%g` with 6 significant digits and Microsoft's three-digit exponents: `0.333333`, `1e+010`, `1e-005`, `1.23457e+008`, `-0`, `1.#INF`.
  - `format` follows C's `printf` (`%5.1f` → `  3.1`, `%e` → `1.234568e+004`).
- **Strings and types:**
  - Adding to a string converts the other side; null becomes `(null : 0x0000000000000000)`.
  - `typeof print` is `native function`.
  - `"abc".tointeger()` gives null; `"x"[0]` is the integer 120.
  - `null == false` and `0 == false` are both false.
  - Arrays have no `find`.
- **Error messages** carry the previous error in brackets: after "cannot index null with 'x'", an arithmetic error reads "arith op + between 'integer' and 'array' (cannot index null with 'x')". Scripts can catch and read this text, so it's reproduced.
- **Constants:** `RAND_MAX` is 32767, `PI` 3.14159.

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
