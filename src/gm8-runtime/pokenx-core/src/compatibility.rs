//! Host-only static compilation. No Game, evaluation, extraction or devices.
//! Results omit names, code, bytecode and private error strings.
use crate::{
    action::Tree,
    gml::{
        Compiler,
        runtime::{ArrayAccessor, Error, InstanceIdentifier, Instruction, Node},
    },
};
use gm8exe::{GameAssets, asset::extension::FileKind};

#[derive(Default)]
pub(crate) struct ErrorCounts {
    total: usize,
    unknown: usize,
}
impl ErrorCounts {
    fn error(&mut self, error: &Error) {
        self.total += 1;
        self.unknown += usize::from(matches!(error, Error::UnknownFunction(_)));
    }

    fn array(&mut self, array: &ArrayAccessor) {
        match array {
            ArrayAccessor::None => (),
            ArrayAccessor::Single(node) => self.node(node),
            ArrayAccessor::Double(a, b) => {
                self.node(a);
                self.node(b);
            },
        }
    }

    fn owner(&mut self, owner: &InstanceIdentifier) {
        if let InstanceIdentifier::Expression(node) = owner {
            self.node(node);
        }
    }

    pub(crate) fn node(&mut self, node: &Node) {
        match node {
            Node::Constant { .. } | Node::Literal { .. } => (),
            Node::Variable { accessor } => {
                self.array(&accessor.array);
                self.owner(&accessor.owner);
            },
            Node::Field { accessor } => {
                self.array(&accessor.array);
                self.owner(&accessor.owner);
            },
            Node::ContextFunction { args, .. }
            | Node::StateFunction { args, .. }
            | Node::RoutineFunction { args, .. }
            | Node::ValueFunction { args, .. }
            | Node::Script { args, .. }
            | Node::ExtensionFunction { args, .. } => {
                for arg in args {
                    self.node(arg);
                }
            },
            Node::Unary { child, .. } => self.node(child),
            Node::Binary { left, right, .. } => {
                self.node(left);
                self.node(right);
            },
            Node::RuntimeError { error } => self.error(error),
        }
    }

    pub(crate) fn instructions(&mut self, instructions: &[Instruction]) {
        for instruction in instructions {
            match instruction {
                Instruction::SetField { accessor, value } => {
                    self.array(&accessor.array);
                    self.owner(&accessor.owner);
                    self.node(value);
                },
                Instruction::SetVariable { accessor, value } => {
                    self.array(&accessor.array);
                    self.owner(&accessor.owner);
                    self.node(value);
                },
                Instruction::EvalExpression { node } => self.node(node),
                Instruction::SetReturnValue { value } => self.node(value),
                Instruction::IfElse { cond, if_body, else_body } => {
                    self.node(cond);
                    self.instructions(if_body);
                    self.instructions(else_body);
                },
                Instruction::LoopUntil { cond, body } | Instruction::LoopWhile { cond, body } => {
                    self.node(cond);
                    self.instructions(body);
                },
                Instruction::LoopFor { cond, body, step } => {
                    self.node(cond);
                    self.instructions(body);
                    self.instructions(step);
                },
                Instruction::Repeat { count, body } => {
                    self.node(count);
                    self.instructions(body);
                },
                Instruction::With { target, body } => {
                    self.node(target);
                    self.instructions(body);
                },
                Instruction::Switch { input, cases, body, .. } => {
                    self.node(input);
                    for (node, _) in cases {
                        self.node(node);
                    }
                    self.instructions(body);
                },
                Instruction::RuntimeError { error } => self.error(error),
                Instruction::Return { .. } | Instruction::GlobalVar { .. } => (),
            }
        }
    }
}

#[derive(Debug, Default)]
pub struct CompilationSummary {
    pub scripts: usize,
    pub script_errors: usize,
    pub action_trees: usize,
    pub action_errors: usize,
    pub code_fragments: usize,
    pub code_errors: usize,
    pub constant_expressions: usize,
    pub constant_errors: usize,
    pub extension_files_not_compiled: usize,
    pub embedded_runtime_errors: usize,
    pub unknown_function_nodes: usize,
}

pub fn compile_assets(assets: &GameAssets) -> CompilationSummary {
    let mut compiler = Compiler::for_game_assets(assets);
    let mut result = CompilationSummary::default();
    let mut errors = ErrorCounts::default();
    for script in assets.scripts.iter().flatten() {
        result.scripts += 1;
        match compiler.compile_game_script(&script.name.0, &script.source.0) {
            Ok(code) => errors.instructions(&code),
            Err(_) => result.script_errors += 1,
        }
    }
    for object in assets.objects.iter().flatten() {
        for event in object.events.iter().flatten() {
            result.action_trees += 1;
            match Tree::from_list(&event.1, &mut compiler) {
                Ok(tree) => tree.audit_compilation(&mut errors),
                Err(_) => result.action_errors += 1,
            }
        }
    }
    for timeline in assets.timelines.iter().flatten() {
        for moment in &timeline.moments {
            result.action_trees += 1;
            match Tree::from_list(&moment.1, &mut compiler) {
                Ok(tree) => tree.audit_compilation(&mut errors),
                Err(_) => result.action_errors += 1,
            }
        }
    }
    let mut code = |source: &[u8]| {
        if !source.is_empty() {
            result.code_fragments += 1;
            match compiler.compile(source) {
                Ok(code) => errors.instructions(&code),
                Err(_) => result.code_errors += 1,
            }
        }
    };
    for room in assets.rooms.iter().flatten() {
        code(&room.creation_code.0);
        for instance in &room.instances {
            code(&instance.creation_code.0);
        }
    }
    for trigger in assets.triggers.iter().flatten() {
        code(&trigger.condition.0);
    }
    for initializer in &assets.library_init_strings {
        code(&initializer.0);
    }
    for extension in &assets.extensions {
        for file in &extension.files {
            if matches!(file.kind, FileKind::GmlScript | FileKind::ActionLibrary) {
                result.extension_files_not_compiled += 1;
            }
            for constant in &file.consts {
                result.constant_expressions += 1;
                match compiler.compile_expression(&constant.value.0) {
                    Ok(node) => errors.node(&node),
                    Err(_) => result.constant_errors += 1,
                }
            }
        }
    }
    for constant in &assets.constants {
        result.constant_expressions += 1;
        match compiler.compile_expression(&constant.expression.0) {
            Ok(node) => errors.node(&node),
            Err(_) => result.constant_errors += 1,
        }
    }
    result.embedded_runtime_errors = errors.total;
    result.unknown_function_nodes = errors.unknown;
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::title_tests::fixture;

    #[test]
    fn fixture_compiles_statically_and_reports_invalid_code_without_source() {
        let mut assets = gm8exe::reader::from_exe(fixture::executable(), None::<fn(&str)>, true, false).unwrap();
        let valid = compile_assets(&assets);
        assert_eq!(valid.scripts, 1);
        assert_eq!(valid.action_trees, 3);
        assert_eq!(valid.script_errors + valid.action_errors + valid.code_errors + valid.constant_errors, 0);
        assets.scripts[0].as_mut().unwrap().source = "private_secret_name = (".into();
        assets.rooms[0].as_mut().unwrap().creation_code = "private_room = (".into();
        assets.constants.push(gm8exe::asset::Constant { name: "private_constant".into(), expression: "(".into() });
        let invalid = compile_assets(&assets);
        assert_eq!(invalid.script_errors, 1);
        assert_eq!(invalid.code_errors, 1);
        assert_eq!(invalid.constant_errors, 1);
        assert!(!format!("{invalid:?}").contains("private_"));
        assets.scripts[0].as_mut().unwrap().source = "if (true) a[private_index()] = private_value();".into();
        assets.rooms[0].as_mut().unwrap().creation_code = "return private_return();".into();
        assets.constants[0].expression = "private_constant_call()".into();
        let embedded = compile_assets(&assets);
        assert_eq!(embedded.script_errors + embedded.code_errors + embedded.constant_errors, 0);
        assert_eq!(embedded.embedded_runtime_errors, 4);
        assert_eq!(embedded.unknown_function_nodes, 4);
        assert!(!format!("{embedded:?}").contains("private_"));
    }

    #[test]
    fn shared_registration_preserves_precedence_sparse_ids_and_constant_offsets() {
        use gm8exe::asset::{
            Constant,
            extension::{Extension, File, FileConst},
        };
        let mut assets = gm8exe::reader::from_exe(fixture::executable(), None::<fn(&str)>, true, false).unwrap();
        let mut object = assets.objects.pop().unwrap().unwrap();
        object.name = "shared".into();
        assets.objects = vec![None, None, None, None, Some(object)];
        assets.sprites[0].as_mut().unwrap().name = "shared".into();
        let script = assets.scripts.pop().unwrap();
        assets.scripts = vec![None, script];
        assets.extensions.push(Extension {
            name: "extension".into(),
            folder_name: "folder".into(),
            files: vec![File {
                name: "file".into(),
                kind: FileKind::Other,
                initializer: "".into(),
                finalizer: "".into(),
                functions: vec![],
                consts: vec![FileConst { name: "extension_constant".into(), value: "1".into() }],
                contents: Box::default(),
            }],
        });
        assets.constants.push(Constant { name: "user_constant".into(), expression: "2".into() });
        let mut compiler = Compiler::for_game_assets(&assets);
        match compiler.compile_expression(b"shared").unwrap() {
            Node::Literal { value: crate::gml::Value::Real(value) } => assert_eq!(value.to_i32(), 4),
            _ => panic!("object constant must win over sprite"),
        }
        assert!(matches!(compiler.compile_expression(b"fixture_clamp(1,2,3)").unwrap(), Node::Script {
            script_id: 1,
            ..
        }));
        assert!(matches!(compiler.compile_expression(b"extension_constant").unwrap(), Node::Constant {
            constant_id: 0
        }));
        assert!(matches!(compiler.compile_expression(b"user_constant").unwrap(), Node::Constant { constant_id: 1 }));
    }
}
