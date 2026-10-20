//! Small, bounded, `no_std` compiler and stack VM for a deliberately narrow
//! GML subset. This is a portability slice, not OpenGMK's game runtime.
use alloc::{string::String, vec::Vec};
use gml_parser::{
    ast::{self, Expr},
    token::Operator,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Limits {
    pub max_source_bytes: usize,
    pub max_instructions: usize,
    pub max_steps: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Self { max_source_bytes: 64 * 1024, max_instructions: 16 * 1024, max_steps: 64 * 1024 }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Instruction<'a> {
    Constant(f64),
    Load(&'a [u8]),
    Store(&'a [u8]),
    Add,
    Subtract,
    Multiply,
    Divide,
    Modulo,
    Negate,
    Not,
    Equal,
    NotEqual,
    Less,
    LessEqual,
    Greater,
    GreaterEqual,
    Pop,
    Return,
}

/// Compiled code borrows identifiers from the source buffer. Keep that buffer
/// alive for at least as long as this program.
#[derive(Clone, Debug, PartialEq)]
pub struct Program<'a> {
    instructions: Vec<Instruction<'a>>,
    max_steps: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CompileError {
    SourceTooLarge,
    TooManyInstructions,
    Parse(String),
    UnsupportedExpression,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RuntimeError {
    StepLimitExceeded,
    StackUnderflow,
    MissingReturnValue,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Execution {
    pub return_value: f64,
    /// Scalar variables assigned during this run. Unassigned variables read as 0.
    pub variables: Vec<(Vec<u8>, f64)>,
}

/// Compile simple scalar assignments, arithmetic/comparisons, `return`, and
/// expression statements into a bounded portable bytecode.
pub fn compile<'a>(source: &'a [u8], limits: Limits) -> Result<Program<'a>, CompileError> {
    if source.len() > limits.max_source_bytes {
        return Err(CompileError::SourceTooLarge);
    }
    let ast = ast::AST::new(source).map_err(|e| CompileError::Parse(e.message))?;
    let mut compiler = Compiler { code: Vec::new(), max: limits.max_instructions };
    for expr in ast.iter() {
        compiler.statement(expr)?;
    }
    if compiler.code.len() > limits.max_instructions {
        return Err(CompileError::TooManyInstructions);
    }
    Ok(Program { instructions: compiler.code, max_steps: limits.max_steps })
}

impl Program<'_> {
    pub fn instruction_count(&self) -> usize {
        self.instructions.len()
    }

    /// Execute the supported scalar bytecode. The VM intentionally has no host
    /// calls, asset access, event dispatch, or graphics/audio side effects.
    pub fn run(&self) -> Result<Execution, RuntimeError> {
        let mut stack = Vec::<f64>::new();
        let mut variables: Vec<(Vec<u8>, f64)> = Vec::new();
        let mut steps = 0usize;
        for op in &self.instructions {
            steps = steps.saturating_add(1);
            if steps > self.max_steps {
                return Err(RuntimeError::StepLimitExceeded);
            }
            match *op {
                Instruction::Constant(x) => stack.push(x),
                Instruction::Load(name) => stack.push(get_var(&variables, name)),
                Instruction::Store(name) => {
                    let value = stack.pop().ok_or(RuntimeError::StackUnderflow)?;
                    if let Some((_, old)) = variables.iter_mut().find(|(n, _)| n.as_slice() == name) {
                        *old = value;
                    } else {
                        variables.push((name.to_vec(), value));
                    }
                },
                Instruction::Negate => {
                    let x = stack.pop().ok_or(RuntimeError::StackUnderflow)?;
                    stack.push(-x);
                },
                Instruction::Not => {
                    let x = stack.pop().ok_or(RuntimeError::StackUnderflow)?;
                    stack.push(boolean(x == 0.0));
                },
                Instruction::Pop => {
                    stack.pop().ok_or(RuntimeError::StackUnderflow)?;
                },
                Instruction::Return => {
                    return Ok(Execution {
                        return_value: stack.pop().ok_or(RuntimeError::MissingReturnValue)?,
                        variables,
                    })
                },
                op => {
                    let rhs = stack.pop().ok_or(RuntimeError::StackUnderflow)?;
                    let lhs = stack.pop().ok_or(RuntimeError::StackUnderflow)?;
                    stack.push(match op {
                        Instruction::Add => lhs + rhs,
                        Instruction::Subtract => lhs - rhs,
                        Instruction::Multiply => lhs * rhs,
                        Instruction::Divide => lhs / rhs,
                        Instruction::Modulo => lhs % rhs,
                        Instruction::Equal => boolean(lhs == rhs),
                        Instruction::NotEqual => boolean(lhs != rhs),
                        Instruction::Less => boolean(lhs < rhs),
                        Instruction::LessEqual => boolean(lhs <= rhs),
                        Instruction::Greater => boolean(lhs > rhs),
                        Instruction::GreaterEqual => boolean(lhs >= rhs),
                        _ => return Err(RuntimeError::StackUnderflow),
                    });
                },
            }
        }
        Ok(Execution { return_value: 0.0, variables })
    }
}

fn boolean(value: bool) -> f64 {
    if value { 1.0 } else { 0.0 }
}
fn get_var(vars: &[(Vec<u8>, f64)], name: &[u8]) -> f64 {
    vars.iter().find(|(n, _)| n.as_slice() == name).map(|(_, v)| *v).unwrap_or(0.0)
}

struct Compiler<'a> {
    code: Vec<Instruction<'a>>,
    max: usize,
}
impl<'a> Compiler<'a> {
    fn emit(&mut self, op: Instruction<'a>) -> Result<(), CompileError> {
        if self.code.len() >= self.max {
            return Err(CompileError::TooManyInstructions);
        }
        self.code.push(op);
        Ok(())
    }

    fn statement(&mut self, expr: &Expr<'a>) -> Result<(), CompileError> {
        match expr {
            Expr::Binary(b) if b.op == Operator::Assign => {
                let Expr::LiteralIdentifier(name) = &b.left else { return Err(CompileError::UnsupportedExpression) };
                self.expression(&b.right)?;
                self.emit(Instruction::Store(name))
            },
            Expr::Return(value) => {
                self.expression(value)?;
                self.emit(Instruction::Return)
            },
            Expr::Group(items) => {
                for item in items {
                    self.statement(item)?;
                }
                Ok(())
            },
            _ => {
                self.expression(expr)?;
                self.emit(Instruction::Pop)
            },
        }
    }

    fn expression(&mut self, expr: &Expr<'a>) -> Result<(), CompileError> {
        match expr {
            Expr::LiteralReal(x) => self.emit(Instruction::Constant(*x)),
            Expr::LiteralIdentifier(name) => self.emit(Instruction::Load(name)),
            Expr::Group(items) if items.len() == 1 => self.expression(&items[0]),
            Expr::Unary(u) => {
                self.expression(&u.child)?;
                match u.op {
                    Operator::Subtract => self.emit(Instruction::Negate),
                    Operator::Not => self.emit(Instruction::Not),
                    _ => Err(CompileError::UnsupportedExpression),
                }
            },
            Expr::Binary(b) => {
                self.expression(&b.left)?;
                self.expression(&b.right)?;
                let op = match b.op {
                    Operator::Add => Instruction::Add,
                    Operator::Subtract => Instruction::Subtract,
                    Operator::Multiply => Instruction::Multiply,
                    Operator::Divide => Instruction::Divide,
                    Operator::Modulo => Instruction::Modulo,
                    Operator::Equal => Instruction::Equal,
                    // Assignment is only supported as a top-level statement
                    // where the left operand is checked as an identifier.
                    Operator::Assign => return Err(CompileError::UnsupportedExpression),
                    Operator::NotEqual => Instruction::NotEqual,
                    Operator::LessThan => Instruction::Less,
                    Operator::LessThanOrEqual => Instruction::LessEqual,
                    Operator::GreaterThan => Instruction::Greater,
                    Operator::GreaterThanOrEqual => Instruction::GreaterEqual,
                    _ => return Err(CompileError::UnsupportedExpression),
                };
                self.emit(op)
            },
            _ => Err(CompileError::UnsupportedExpression),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compiles_and_runs_scalar_assignments_and_return() {
        let source = b"x = 6 * 7; y = x + 1; return y;";
        let program = compile(source, Limits::default()).unwrap();
        let result = program.run().unwrap();
        assert_eq!(result.return_value, 43.0);
        assert_eq!(get_var(&result.variables, b"x"), 42.0);
        assert_eq!(program.instruction_count(), 10);
    }

    #[test]
    fn supports_comparison_and_unary_minus() {
        let program = compile(b"return -2 < 1;", Limits::default()).unwrap();
        assert_eq!(program.run().unwrap().return_value, 1.0);
    }

    #[test]
    fn rejects_features_outside_the_slice() {
        assert_eq!(
            compile(b"return show_message(1);", Limits::default()).unwrap_err(),
            CompileError::UnsupportedExpression
        );
        let mut limits = Limits::default();
        limits.max_source_bytes = 2;
        assert_eq!(compile(b"return 1;", limits).unwrap_err(), CompileError::SourceTooLarge);
    }

    #[test]
    fn enforces_instruction_limit() {
        let mut limits = Limits::default();
        limits.max_instructions = 2;
        assert_eq!(compile(b"return 1+2;", limits).unwrap_err(), CompileError::TooManyInstructions);
    }
}
