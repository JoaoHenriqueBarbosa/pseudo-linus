//! O `Store` concreto do transpilador: um vetor por tipo de nó, de lista e de payload, declarado com
//! `define_store!` (seção "Arena tipada" do README). No upstream são o `expr::data::Store`, o `stmt::data::Store` e
//! o bump do parser; aqui é um dono só por arquivo analisado.

use crate::{B, E, G, S};

crate::define_store! {
    /// Os nós de um arquivo analisado.
    pub struct Store {
        // E
        e_name_of_symbol: E::NameOfSymbol,
        e_array: E::Array,
        e_class: G::Class,
        e_unary: E::Unary,
        e_binary: E::Binary,
        e_new: E::New,
        e_function: E::Function,
        e_call: E::Call,
        e_dot: E::Dot,
        e_index: E::Index,
        e_arrow: E::Arrow,
        e_jsx_element: E::JSXElement,
        e_big_int: E::BigInt,
        e_object: E::Object,
        e_spread: E::Spread,
        e_template: E::Template,
        e_reg_exp: E::RegExp,
        e_await: E::Await,
        e_yield: E::Yield,
        e_if: E::If,
        e_import: E::Import,
        e_inlined_enum: E::InlinedEnum,
        e_string: E::EString,
        e_rope: E::Rope,
        // S
        s_block: S::Block,
        s_break: S::Break,
        s_class: S::Class,
        s_comment: S::Comment,
        s_continue: S::Continue,
        s_directive: S::Directive,
        s_do_while: S::DoWhile,
        s_enum: S::Enum,
        s_export_clause: S::ExportClause,
        s_export_default: S::ExportDefault,
        s_export_equals: S::ExportEquals,
        s_export_from: S::ExportFrom,
        s_export_star: S::ExportStar,
        s_expr: S::SExpr,
        s_for_in: S::ForIn,
        s_for_of: S::ForOf,
        s_for: S::For,
        s_function: S::Function,
        s_if: S::If,
        s_import: S::Import,
        s_label: S::Label,
        s_local: S::Local,
        s_namespace: S::Namespace,
        s_return: S::Return,
        s_switch: S::Switch,
        s_throw: S::Throw,
        s_try: S::Try,
        s_while: S::While,
        s_with: S::With,
        s_lazy_export: crate::ExprData,
        // B
        b_identifier: B::Identifier,
        b_array: B::Array,
        b_object: B::Object,
        // Listas
        exprs: crate::Expr,
        stmts: crate::Stmt,
        g_properties: G::Property,
        g_args: G::Arg,
        g_decls: G::Decl,
        g_class_static_blocks: G::ClassStaticBlock,
        clause_items: crate::ClauseItem,
        enum_values: crate::EnumValue,
        cases: crate::Case,
        array_bindings: crate::ArrayBinding,
        b_properties: B::Property,
        template_parts: E::TemplatePart,
        // Escopos e os mapas de namespace do TypeScript
        scopes: crate::Scope,
        scope_orders: crate::scope::ScopeOrder,
        // Os usos de import() dinâmico por registro (`DynamicImportUse`)
        dynamic_import_aliases: crate::StoreStr,
        dynamic_import_items: crate::ast_result::DynamicImportItem,
        ts_namespace_scopes: crate::TSNamespaceScope,
        ts_namespace_member_maps: crate::TSNamespaceMemberMap,
    }
}

impl AsMut<Store> for Store {
    #[inline]
    fn as_mut(&mut self) -> &mut Store {
        self
    }
}
