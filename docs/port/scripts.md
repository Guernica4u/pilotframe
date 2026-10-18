# The game's scripts (Squirrel)

What we know about how Titanfall runs its Squirrel scripts, from the scripts in the EA install and the DLLs (`launcher.dll`, `engine.dll`, `client.dll`). Our interpreter is `crates/pf-squirrel`.

## Where things are

- **The scripts:** 481 `.nut` files (7.3 MB) under `scripts/vscripts/` in `englishclient_mp_common.bsp.pak000_dir.vpk`, plus 75 `.menu` and the `.res` layouts.
- **The interpreter:** **Squirrel 2.2.3 stable** (its version string), built into `launcher.dll`. `dedicated.dll` carries a copy.
- **No server code:** the PC game ships no `server.dll` (multiplayer ran on Respawn's servers), so the server VM's native functions aren't in the install. Where server scripts need one, its behaviour has to be worked out from the scripts and the client's matching code. We run the server scripts ourselves (owner's decision 2026-10-17): see [server.md](server.md).

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
- **Run 2** (`game_results_run2.txt`):
  - **Table iteration order is the hash order,** and scripts see it. Examples:
    - inserting `a`..`h` iterates as `acbedgfh`;
    - keys `key0`..`key19` come out as 1,0,3,2,5,4,7,6,9,8,10,11,15,14,13,12,19,18,17,16;
    - the integer keys 0, 7, 14…273 come out as 0, 259, 196, 133, 70, 7, 266…;
    - a table literal `{ zebra, apple, mango, kiwi, banana }` iterates banana, apple, kiwi, zebra, mango.

    Our tables must reproduce these exactly. The rules that do it are under "Inside the interpreter" below.
  - **Arity is strict:** calling with too few or too many arguments throws "wrong number of parameters (calling (unnamed_function)): found 1, expected 2". The counts include `this`.
  - **Class defaults are shared:** a class member's default array is the same array in every instance.
  - **Assignment needs an existing slot:** `t.b = 1` on a missing slot throws "the index 'b' does not exist"; `<-` creates it or overwrites it.
  - `clone` is shallow.
  - `null` can't be a key ("null cannot be used as index").
  - `yield` isn't a keyword; `static` and `extends` work.
- **Parser check against the game:** `crates/pf-squirrel/tests/oracle_cases.rs` makes our parser accept every case the game compiled and reject every case it refused, with the same message. All 146 agree.

**Parser check:** all 481 scripts parse (`crates/pf-squirrel/tests/game_scripts.rs`, which runs when the install is present).

## Inside the interpreter (read from launcher.dll)

### Value tags

Each value is a 4-byte type tag plus 8 bytes of data. Tags seen so far: null `0x1000001`, bool `0x1000008`, integer `0x5000002`, float `0x5000004`, string `0x8000010`, table `0xA000020`, class `0x8004000`, instance `0xA008000`. Bit 27 (`0x8000000`) marks a reference-counted object.

### Strings

- Every string is interned in one VM-wide string table, so equal strings are the same object; table keys compare by tag and object.
- **Hash** (64-bit, unsigned, computed once when the string is made), with `n` the byte length:
  - `h = n`; `step = (n | 32) >> 5` (1 for strings under 32 bytes);
  - walk the bytes **forward from the first**, taking every `step`-th byte while the bytes remaining are at least `step`;
  - for each byte `c` (read as a signed char, then masked to 16 bits): `h = h ^ (h*32 + (h >> 2) + c)`.

### Tables

- **Key hash:** integers and bools use their value, floats their value truncated to an integer, strings the hash above, and any other object its address shifted right by 3. The slot is `hash & (size - 1)`.
- **Layout:** an array of `size` nodes (key, value, next-in-chain), `size` a power of two, plus a "free" cursor that starts at the last node.
- **Size:** a new table starts at 4, or at the smallest power of two ≥ the requested size. A table literal requests its entry count (so a five-entry literal starts at 8).
- **Insert** (a missing key): look up its main slot.
  - If the main slot is empty, the key goes there.
  - If it holds a key whose own main slot is elsewhere, and the free cursor is below the main slot, that key is moved to the free node (its chain relinked) and the new key takes the main slot.
  - Otherwise the new key goes in the free node, linked right after the main slot.
  - Then the free cursor walks down to the next node with no key and no link. If it runs off the bottom, the table resizes and the insert is retried.
- **Resize:** with `n` = current size (counted as at least 4) and `c` = keys held:
  - `c ≥ n − n/4`: double;
  - `c ≤ n/4` and `n > 4`: halve;
  - otherwise the same size (only when forced, i.e. on a full insert; a delete doesn't resize then).

  Old nodes are re-inserted in node order, first to last.
- **Delete:** clears the node's key and value, then checks for a resize (which may shrink).
- **Iteration order** (`foreach`) and `clone` both walk the nodes first to last.
- **Checked:** these rules reproduce all five key orders the game printed in oracle run 2 (insert order, `key0`..`key19`, the multiples of 7, the five-entry literal, and deletes followed by an insert).

### Calls

- **Argument count** (counts include `this`), with `n` the function's parameters:
  - fewer than `n`: allowed if the function has default values and the missing ones are all covered by them;
  - more than `n`: allowed if the function takes `...` (the extras go to `vargv`);
  - otherwise: "wrong number of parameters (calling NAME): found F, expected N", with "(unnamed_function)" for a function without a name.

### Errors

- **The last error is kept** in the VM and never cleared, not even by `catch`.
- **Arithmetic errors include it:** "arith op + between 'integer' and 'array'", followed by " (LAST ERROR)" when there is one. Only the arithmetic error does this (oracle E001–E005). What it shows when the last error isn't a string is still to be checked (oracle E009).
- **Indexing a missing key:** arrays, tables, classes, instances (and userdata) say "the index 'KEY' does not exist"; other types say "cannot index TYPE with 'KEY'". The key is cut to 50 characters.

### Sorting

`array.sort` is a quicksort: the first element is the pivot; scan up from the left past elements that compare ≤ 0 with the pivot, down from the right past elements that compare > 0; swap and repeat until the scans cross, then swap the pivot into place and sort both sides. A comparison function gets (element, pivot) with the root table as `this`. Where equal elements end up depends on this, and scripts can see it (oracle A013/A014 check it).

### Threads and signals

launcher.dll's thread system, which `_threads.nut` builds on (our `threads.rs`):

- **The keywords are calls.** The compiler looks these names up in the root table and calls them: `thread f(a)` → `SpinOff`, `waitthread` → `SpinOffAndWaitFor`, `waitthreadsolo` → `SpinOffAndWaitForSolo`, `wait t` → `Wait`, `delaythread(d) f(a)` → `SpinOffDelayed(d, f, a)`. A script that replaced one of them would change the keyword. We pass `a` as `this` for `thread a.f()` (to be confirmed, oracle T010).
- **`SpinOff(f, ...)`** makes a new script thread whose `this` is `SpinOff`'s `this`, then runs it at once until it first waits. With a delay (`SpinOffDelayed`), the thread waits first and starts when its time comes.
- **Waiting by time:** `Wait(t)` puts the thread in the bucket for time `now + t` (32-bit float). Buckets are kept in time order; threads join a bucket at the end.
- **Each frame** (`RunFrame(time)`):
  1. threads woken outside the last frame become the ready list; if there are none, the end-of-frame list does;
  2. every bucket whose time has come moves onto the end of the ready list, in time order (a thread in `WaitSignalTimeout` stops waiting for its signals);
  3. the ready list runs in rounds: each round takes the whole list and runs it in order; threads made ready during a round run in the next round, in the same frame; when nothing is ready, the `WaitEndFrame` threads become ready.

  So `wait 0` waits until the next frame.
- **Waking:** a thread woken during a frame joins the ready list; one woken outside a frame (from engine code) joins a list that runs first next frame.
- **`waitthread`:** if the new thread is still running after its first run, the caller waits for it. When it ends, the caller wakes. If the caller ends first, the child ends too (not with `waitthreadsolo`).
- **`Signal(owner, name[, results])`:**
  - the owner must be an object;
  - the name must have been registered with `RegisterSignal` ("\"X\" is not a registered signal");
  - `results` defaults to a new table, and gets `results.signal = name`;
  - then every thread that called `EndSignal(owner, name)` ends;
  - then every thread in `WaitSignal(owner, name, ...)` wakes, and its `WaitSignal` returns `results`.
- **Ending a thread:**
  - its waiting parent wakes;
  - a `waitthread` child ends too;
  - its signal waits are dropped;
  - its `OnThreadEnd` functions run in order, outside any thread, bound to the `this` they were registered with.

  A thread that ends itself (by signalling its own end signal) stops at that call.
- **Not ported yet:**
  - the dead-owner rules: `WaitSignal`/`EndSignal` on an entity already destroyed, with `"OnDestroy"` among the names, return or end at once;
  - a second hook in `Signal` whose meaning isn't clear yet;
  - the name of one results key set in the dead-owner case.

  These need entities first.

### Startup and the compiler's extras

- **The VM's own scripts:**
  - launcher.dll creates a VM and runs `init.nut`, `_consts.nut` and `_threads.nut` from the VPK;
  - it registers its natives (base library, threads, `Vector`, plus `developer`, `GetFunctionSignature`, `IsValid`).
  - launcher.dll also carries an embedded copy of an older `init.nut` (with a script `Assert`), but nothing references it: it's dead data. The VPK's copy says "Reference script only" yet is the one that runs.
- **The client VM** (client.dll):
  1. creates the VM;
  2. registers its native groups and constants (`COCKPIT_PANEL_*`);
  3. runs `vscript_client.nut`;
  4. includes `_cl_mapspawn`.
- **`Assert` is a compiler keyword:** an `Assert( ... )` statement compiles to nothing (the compiler skips to the matching `)`), so the shipped game never evaluates asserts.
- **`IncludeFile( name )`** (in `_threads.nut`):
  - runs the file in a new scope table that delegates to the root table;
  - then runs its `main()` as a thread with that scope as `this`, through `SpinOff.bindenv( scope )`.

  So `bindenv` works on native functions too.
- **`developer()`** returns the developer console variable (0 in the shipped game).
- **`IsValid( x )`:** null is false; an instance is true only if it's a live engine entity; anything else is true.

### Vector

A class whose instances hold three 32-bit floats (our `vector.rs`).
- **Construction and fields:** missing or non-number constructor arguments are 0. `_get`/`_set` serve `x`, `y`, `z`; any other key is "the index 'K' does not exist".
- **Type and printing:** `typeof` is `"Vector"`; it prints as `(vector : (%f, %f, %f))`; `foreach` gives x, y, z.
- **Arithmetic:** `+` and `-` take Vectors; `*` takes a number ("Tried to scale vector by something other than a number"). `==` compares identity.
- **Methods:**
  - `Length`, `LengthSqr`, `Length2D`, `Length2DSqr`, `Dot`, `Cross`;
  - `Norm` and `Normalize` (in place, dividing by length + FLT_MIN, returning the old length);
  - `GetAngles` (direction to pitch/yaw in [0, 360));
  - `AnglesToForward`, `AnglesToRight`, `AnglesToUp`;
  - `AnglesCompose`, `AnglesInverse`, `AnglesLerp` (through quaternions: multiply with alignment, conjugate over the squared norm, slerp);
  - `RotateAngles` (this vector times the argument's rotation matrix);
  - `ToKVString`.
- **The game's sine/cosine:** most angle math uses its own SSE approximation, not the C library's:
  - the angle in turns, folded into a quarter turn;
  - then an odd degree-9 polynomial, with coefficients 39.536705, −76.549782, 81.601006, −41.341656, 6.283185 (just below 2π) from launcher.dll's data;
  - cosine is the sine of angle + π/2.

  `AnglesToForward` and the slerp use the C library's instead.
- **Checked against the game:** pending (oracle V001–V018, printed to 9 digits to catch rounding).

## Our interpreter (`crates/pf-squirrel`)

- **Pipeline:** our parser, then our own compiler to a simple stack bytecode, then our VM. The bytecode is ours; what matches the game is the behaviour.
- **Numbers:** 32-bit integers (wrapping) and 32-bit floats, printed like the game's C library (`fmt.rs`).
- **Script threads:** each has its own stack, so `wait` can stop it mid-function; scheduling is the game's (above), run by `Vm::run_frame(time)`. A call from Rust (an engine callback) must finish without waiting.
- **Checked:**
  - all 146 oracle cases print exactly what the game printed (`tests/oracle_run.rs`);
  - all 481 game scripts compile (`tests/game_scripts.rs`, when the install is present).
- **One compiler quirk ported:** Squirrel 2's compiler only checks that the last operand of an assignment's left side is assignable, so `local x = 0 + y -= w` compiles (one game script, `cl_burncards_lobby.nut`, has it). We give `(0 + y) - w` with `y` unchanged, from how that compiler works; oracle Q001/Q002 will confirm.
- **Not yet checked against the game** (oracle run 3 cases are written): rounding of exact ties in `format` (assumed away from zero, the old Microsoft library), not-a-number printing, `%s` with numbers, the error texts for natives called with the wrong argument count, and the thread and signal rules (oracle T001–T011).
- **Survey tool:** `cargo run -p pf-squirrel --example client_vm` runs the client VM's startup from the install. Each engine function the scripts call that we don't have yet gets a stand-in returning null; the tool lists them in order of first use.
- **Next:** the client.dll natives the survey lists (`IsClient`, `IsMultiplayer`, `GetMapName`, the `Remote` object…), each ported from client.dll.

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

### Argument checks

launcher.dll registers each client.dll function with Squirrel's own argument check (`sq_setparamscheck`), so a wrong argument fails before the function runs:
- **The type mask** is built from the function's parameter types: `.` for `this`, then per parameter `n` (int or float), `x` (an instance: Vector), `b` (bool), `s` (string), `.` (any script object). Another type ends the mask there; the parameters after it aren't checked. The parameter count must be exact (types + 1).
- Functions that read the Squirrel stack themselves (`Rumble_CreateGraph`, `GetPersistentVarAsInt`, the HUD's `SetColor`…) carry their own mask and count instead (e.g. `.saaaa`, 6; `..`, at least 2).
- **The errors** are Squirrel's: "wrong number of parameters: found F, expected N"; "parameter I has an invalid type 'T' ; expected: 'A|B'" (I counts `this` as 0; the expected names come from the mask's type bits, lowest first, so `n` reads "integer|float").
- **Then the values are converted:** an int passed for a float becomes a float, a float passed for an int is truncated, a non-bool for a bool is false.
- **Errors raised inside a function** (client.dll 0x1802c4fe0) are formatted, lose one trailing line break, and are stored; the last one stored wins and is thrown when the function returns. Whatever the function did before raising stays done.
- Each function's signature comes from a small template function that writes the return type (+0x38) and appends the parameter types (+0x40); `natives.py` now follows these, so `natives.json` has the parameter list of 504 client functions.
- Ours: `pf-squirrel` has the mask check (`native_checked`, `compile_typemask`); new client functions are registered with their masks. The functions ported before this still need theirs added.

### Client functions (pf-clientvm)

- **`Graph( v, a, b, c, d )`** (0x1802c4480) maps `v` from [a, b] onto [c, d] in 32-bit floats: `(d - c) * ((v - a) / (b - a)) + c`. When a == b (or either is not a number) it's a step: d if `v - b >= 0`, else c. `GraphCapped` (0x1802c44d0) clamps the fraction to 0..1 (not-a-number becomes 0). Both were script functions once (`_utility_shared.nut` keeps them commented out, "Moved to code").
- **Model effects** (`ModelFX_*`, 0x18015a4e0 on): a definition per model (names matched ignoring ASCII case), holding groups (in the order begun; beginning a name twice makes two groups), each with a start state, who sees it (`all` 0, `friend` 1, `foe` 2, `notLocalPlayer` 3; anything else is an error after the group is made, seen by all) and effects per attachment tag:
  - spawn effect (one per tag; a second replaces it);
  - health effects (health fraction, effect, one-shot flag, sound, one-shot sound), each with a lower fraction than the tag's last, else an error after it's added;
  - health-damage effects (state index, fraction, effect, min/max delay; the looping version has delays -1), the same order rule, its error always named `ModelFX_AddTagHealthDamageFX`;
  - damage effects (state index, effect, min/max delay, sound), break effects (state index, effect, sound), break gibs (state index, model, gib type, min/max velocity).
  - Every addition except the spawn effect marks the model as having state effects; a break effect or gib with state -1 marks a whole-model break. All state effects (damage, health-damage, break) must sit in one group of the model: "ModelFX_AddTagBreakFX: only one group should have body group fx."
  - Effect and sound names are kept to 31 characters, gib models to 63. Adding an effect loads it and keeps its index (0 if not loaded).
  - `ModelFX_BeginData` twice without `ModelFX_EndData` is an error; the add functions without a `BeginData` would crash the game (we raise an error instead, ours). Everything is cleared when the client VM shuts down.
- **Rumble** (`Rumble_CreateGraph` 0x180442300, `Rumble_CreatePlayParams` 0x180440c20): a graph is four arrays of Vectors read as (time, power) points: heavy motor, light motor, left trigger, right trigger. Play params name a graph and may set `position` (default: the engine's "no position", all `FLT_MAX`), `scale`, `scaleMin`, `scaleMax`, `distanceMin`, `distanceMax` (all default 1; ints accepted) and `loop` (a bool, default false). Both are stored by name (case-sensitive, replacing an earlier one). A missing field is skipped without touching the VM's last error; a field of the wrong type is an error naming it.
- **Entity scopes:** `ValidateScriptScope` (0x18002a710) makes an entity's scope on first use by calling init.nut's `VSquirrel_OnCreateScope( key, root )` with the root table as `this`, then sets `self` in it to the entity; it returns true. `GetScriptScope` (0x180027ea0) returns the scope, or null if none was made; the class file wraps it to validate first. The key is launcher.dll's `GenerateUniqueKey` (0x1800099d0): `%x%x%llx_%s` of a random number below 0x1000, the clock in milliseconds, a serial number, and the entity's name (or its class name if it has none). Because of the random number and clock, the root table's order of these keys differs on every run in the game too. Releasing a scope when its entity goes is still to be ported.
- **`GetActiveBurnCardIndex`** (C_BasePlayer, 0x1800368b0) returns the networked field as it is; the class file subtracts 1.

- **`AngleNormalize( a )`** (0x1804fb9f0) wraps to [-180, 180]; **`GetCPULevel`** (0x1802b5b40) is the `particle_cpu_level` console variable; **`PrecacheModel( name )`** (0x1802c5f40): "Empty model name.", "Couldn't precache 'NAME'." (skipped in the optimized lobby); `PerfInitLabel`/`PerfClearAll` do nothing in the shipped game.
- **`Destroy()`** (C_BaseEntity, 0x1802f2eb0): only for entities client script made ("You cannot destroy a server entity in client script"); the entity is released. Destroy callbacks (`SetDoDestroyCallback`) are still to be ported.
- **The connect callback** (0x180087b70): when the local player arrives, once per player, the engine makes the player's scope, looks up `CodeCallback_RunClientConnectScripts` through it and calls it with ( player ) and the root table as `this`. That's where `_cl_mapspawn.nut` creates `level.ent` and starts `Init_ClientScripts`.

- **`CreateClientSidePointCamera( origin, angles, fov )`** (0x1802b5390): a client-only `point_camera` (C_PointCamera); `SetActive`, `SetFOV`, `SetFogEnable` (0x1800d0150 on) only work on client-only cameras ("You cannot call SetFOV() on a server entity"). **`SetVisibleForLocalPlayer`/`SetInvisibleForLocalPlayer( n )`** (0x1802f50c0, 0x1802f5100) set player n's bit in one mask and clear it in the other; client-side entities only. The class file's `Hide()`/`Show()` call them with 0.
- **`SafeareaSettingIsEnabled`** (0x1802c9220): the `cl_safearea` console variable (default "0"). **`CameraPosition`/`CameraAngles`** (C_BasePlayer): the view the game renders from.
- **Built-in signals:** every VM starts with `OnDamaged`, `OnDeath` and `OnDestroy` registered (launcher.dll 0x1800328e0); scripts never register them. (On the server, `EndSignal` on an entity can also register a name the server's entity code accepts; the client passes no such hook.)
- **The HUD root** (`CHudScriptElements`, 0x18042bd50): a full-screen panel named `CHudScriptElements` that loads `resource/UI/HudScripted_mp.res` (`_sp` in single player, `_lobby` in the optimized lobby; 0x18041eb90). `Hud.HudElement( name )` (0x18042a000) wraps a direct child of it ("HudElement "X" could not be found on panel "CHudScriptElements"."), `Hud.GetOrCreateHudElement( name )` (0x18042a890) returns its element if it has one. Its layout is proportional: the file's numbers are in units of a 480-tall screen (we scale by screen height / 480; Source copies the proportional flag from the parent panel; **to confirm with a reference shot**). A `CNestedPanel` loads its children from its `controlSettingsFile`. A panel with `InheritProperties NAME` first gets every key it lacks from the template NAME in the scheme's `InheritableProperties` section (`resource/HudScriptedScheme.res`; 0x1806894b0).

- **Player fields** (C_BasePlayer, networked by the server): `GetCinematicEventFlags` (+0x2a54), `GetXP` (+0x3610), `GetLevel` (the XP table's level for it; "XP N too low" below 0), `HasBadReputation` (+0x2bd8), `GetNextTitanRespawnAvailable` (+0x3638), `GetPlayerSettings` (the settings name), `GetCockpit` (+0x3220, null if none). `HideCrosshairNames`/`UnhideCrosshairNames` set and clear bit 4 of a client-side flags field (+0x3bb0).
- **`GetPersistentVar( path )`** (0x180036810, 0x1802c88a0): only for the local player ("Player must be local."); "Persistent data not available.", "Blank var name not allowed.", "Invalid var name 'P'.", "Specified var is a struct."; a bool, int, float or string comes back as itself; an enum as its item's name, null for the `NULL` item.
- **`SetParentRaw( parent, maintainOffset, blendTime, useHitbox, attachment, hitbox )`** (0x18002ddf0): "Cannot attach to hitbox and standard attachment at same time.", an invalid parent or the entity itself is an error; the offset is kept when maintainOffset is true or the blend time is above 0. (Its model checks and the noclip check wait for models in entity state.)
- **`IsValidInternal()`**: launcher.dll adds it to every registered class (0x18000c920): true while the instance's native object exists.
- **`Enable/DisableHealthChangedCallback`** keep a flag; the `ClientCallback_OnHealthChanged` call comes with entity updates (not ported yet). **`IsWatchingKillReplay`** (0x1802b5970) asks the engine. **`EverythingUnlockedConVarEnabled`** (0x1802c9290): the `everything_unlocked` console variable (default "0").
- **HUD elements:** `SetColor`/`SetColorBG` (0x18042a880, 0x1804298f0) share one reader (0x1804295f0): `( [r, g, b] )`, `( [r, g, b, a] )`, `( [r, g, b], a )`, `( r, g, b )` or `( r, g, b, a )`, each read as an integer and kept to its low 8 bits; a missing alpha is the element's current foreground (or draw) colour's alpha, for either call; wrong shapes are "SetColor/SetColorBG called with invalid argument type; must be array.", "...invalid number of elements in array.", "...invalid number of parameters.". `SetVisGroupID( id )` (0x18041fec0) sets the element's visibility group, -1 until set (0x1804293c0). `EnableKeyBindingIcons` sets the flag docs/port/hud.md describes.
- **Server state stand-in (ours, marked):** until pf-servervm runs a match, `standin.rs` sends the client the same remote calls the server's scripts would (`ServerCallback_SetServerVar` with the var's handle): game state Playing, start time now. The scripts' game-state callbacks expect the HUD to exist, so it runs after the connect step. The survey's host answers every persistent path with 0 (survey only).

### Input

client.dll's input group (0x1802c0eb0):
- **Names:** 137 button names and 4 axis names go into the root table as integers. A button's script ID is its place in client.dll's list (0x180b0c300, entries of {engine button code, name}: `KEY_0`..`KEY_Z`, keypad, punctuation, function keys, `MOUSE_*`, the D-pad, `BUTTON_A`..`BUTTON_STICK_RIGHT`, `STICK1_*`, triggers, `STICK2_*`), not the engine's button code; the axes are `ANALOG_LEFT_X`/`_Y` and `ANALOG_RIGHT_X`/`_Y` (engine axes 0, 1, 4, 3). These lists are in `input.rs`.
- **`InputIsButtonDown( id )`**: controller buttons (engine codes 0x72 to 0x131) count only in controller mode; "Invalid button ID: N". **`InputGetAxis( id )`**: the raw axis over 32767, capped at 1 above (not below -1); 0 while input doesn't go to the game. **`GetKeyBinding( id )`**: the command bound to the button.
- **Callbacks:** each button keeps 8 pressed and 8 released callbacks, each stick 8; each remembers the `this` it was registered with. Registering the same function twice, or a ninth, fails with "Too many button callbacks for button ID N" (the double registration's own message is replaced by that one). Removing one that isn't there: "Specified callback was not registered for button ID N" (axis: "...for axis ID N"; the stick version also wants a script function, "Second argument should be a function"). `RegisterMenuButtonPressed/ReleasedCallback` mark theirs as menu callbacks; `SetInMenuForButtonCallbacks( bool )` picks which kind runs.
- **Each frame** (0x1802c0360): for each button, if its state changed, the pressed (or released) callbacks of the current kind run with ( local player ); if it didn't change but it was pressed and released since the last frame (press times are kept; at most 100 ms back), the pressed then released callbacks run. Then each stick whose value changed calls its callbacks with ( player, value ). The client VM skips buttons and sticks while the game's menus have the input. Then the console-command callbacks run.
- **Console-command callbacks** (`RegisterConCommandTriggeredCallback( command, func )`, 0x1802c0a50): at most 30 in all; the same function twice on a command is "ConCommand 'C' already has this callback registered"; an unknown command "No ConCommand with name 'C' exists". When the command runs, the engine queues it (at most 15 per frame), and the next frame's pass calls its callbacks, newest first, with ( player ) and their own `this`. Deregistering one ("ConCommand 'C' does not has this callback registered" if missing) also drops every queued command.

### Persistence

- **Where the definition comes from:** the persistent data definition (`cfg/server/persistent_player_data_version_N.pdef`: the saved variables and the enums such as `maps`, `gameModes`, `challenge`, `titanDecals`, `burnCard`, `pilotRace`, `unlockRefs`) is read by the **server** (engine.dll 0x180218630 takes the highest version found), which sends it to each client BZ2-compressed with its version number on connect (0x1802188d0). Demos carry a copy (0x180218a20, "pdef source from server"). It is not in the PC install: none of the 286 client VPKs nor the loose files have a `.pdef`.
- **The client functions:** `PersistenceGetEnumCount( enum )` (0x1802c8e30) and `PersistenceGetEnumIndexForItemName( enum, item )` (0x1802c8ea0) ask the engine (engine.dll 0x180211620, 0x180216c50); without a definition they raise "Persistence datadef is not loaded."; an unknown enum is "Unknown persistent enum "NAME"."; an unknown pair is "Invalid enum or item name."; each then returns 0.
- **The plan (owner's decision, 2026-10-17):** our game plays the server's part, so it supplies the definition: rebuilt at runtime from the install's own scripts (every enum and variable they use, item order from the scripts' own lists), with anything that can't be pinned down marked as a placeholder. Nothing of it goes in git.
- **The engine's enum rules** (engine.dll 0x1802170e0, 0x180217620): at most 32 enums and 1,024 item names in all; enum and item names match ignoring ASCII case; a repeated item is refused ("Enum 'E' already has an entry named 'X'."); a first item `NULL` is stored as "" (so index 0 means "none" and looks up as ""). Variable types (0x1802179a0): `int`, `bool`, `float`, `string{N}`, an enum name, or a struct name; an array's size is a number or an enum (its item count). The data block is at most 0x8800 bytes.
- **`PersistenceGetEnumItemNameForIndex( enum, i )`** (0x1802c8f30): "Invalid enum or item index." for an unknown enum or an index outside it. **`PersistenceEnumValueIsValid( enum, item )`** (0x1802c8fc0): an unknown enum is an error ("Unknown persistent enum"), an unknown item is false. **`PersistenceGetArrayCount( path )`** (0x1802c9050): "Invalid item path: "P"." (needs the variables; not ported yet).
- **Enums rebuilt so far** (ours, `pdef.rs`; `cargo run -p pf-clientvm --example pdef_check` lists them and runs the checks):
  - `titanDecals`: `NULL`, then each `CreateDecalData` ref in `_items.nut`, in file order (112). **Checked:** the Titan models' skin families hold the decals in enum order (`_items.nut` uses skin `2 + index` for IMC and `1 + index + count` for Militia): all three Titan models have 227 families (= 1 + 2 × 113), and all 112 decals sit in script order, 110 by texture name (the two co-op decals use textures named `titan_decals_FTveteran` / `FTelite`).
  - `challenge` (203) and `dailychallenge` (185): each `AddChallenge` ref in `_challenges_content.nut` in file order, lower-cased (the script lower-cases them too, "Persistent data is stored as lower case"), split by the category set last (`DAILY` → `dailychallenge`). `challenge` starts with `NULL`: scripts treat index 0 as "none" (`_persistentdata.nut`). **Placeholder:** the `NULL` on `dailychallenge`, and whether challenges the script has commented out were still in the original enums.

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
