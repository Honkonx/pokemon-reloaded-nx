# gm8-script

Bounded GM8.0 script-table reader plus a portable GML subset compiler/VM.

`subset_vm::compile(source, limits)` reuses the OpenGMK `gml-parser` AST and
emits an internal stack bytecode for scalar numeric assignments, arithmetic,
comparisons, unary minus/NOT, expression statements and `return`. `Program::run`
executes only those pure scalar operations. The bytecode borrows identifier
bytes from the source, so retain the source buffer as long as `Program` exists.

This is not OpenGMK's bytecode and does not implement its GML semantics in full.
Calls, strings, arrays, fields, asset constants, control flow, built-ins,
extension functions, events, object variables and host effects are unsupported.
Unsupported AST nodes return `CompileError::UnsupportedExpression`; limits bound
source length, emitted instructions and execution steps. The app's resource
diagnostic uses this API only to report how many scripts fit the subset; it does
not run scripts from a selected game.

The production `gm8emulator::gml::Compiler` remains a separate upstream compiler
that emits the emulator's `Instruction`/`Node` structures. Its runtime still
depends on the desktop `gm8emulator` host and is not linked into the Switch NRO.
