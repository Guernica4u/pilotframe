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

## Our interpreter (`crates/pf-squirrel`)

- **Pipeline:** our parser, then our own compiler to a simple stack bytecode, then our VM. The bytecode is ours; what matches the game is the behaviour.
- **Numbers:** 32-bit integers (wrapping) and 32-bit floats, printed like the game's C library (`fmt.rs`).
- **Script threads:** each has its own stack, so `wait` can stop it mid-function. `thread f()` runs `f` at once until its first wait; `Vm::update(time)` carries on threads whose wait is over. A thread that waits during an update carries on next update at the earliest. A call from Rust (an engine callback) must finish without waiting.
- **Checked:**
  - all 146 oracle cases print exactly what the game printed (`tests/oracle_run.rs`);
  - all 481 game scripts compile (`tests/game_scripts.rs`, when the install is present).
- **One compiler quirk ported:** Squirrel 2's compiler only checks that the last operand of an assignment's left side is assignable, so `local x = 0 + y -= w` compiles (one game script, `cl_burncards_lobby.nut`, has it). We give `(0 + y) - w` with `y` unchanged, from how that compiler works; oracle Q001/Q002 will confirm.
- **Not yet checked against the game** (oracle run 3 cases are written): rounding of exact ties in `format` (assumed away from zero, the old Microsoft library), not-a-number printing, `%s` with numbers, the error texts for natives called with the wrong argument count, and the order threads carry on in within one update.
- **Next:** the native functions (thread signals, `Vector`, then what `init.nut`, `_consts.nut`, `_threads.nut` and the client VM's scripts call), then running the client VM's startup.

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
