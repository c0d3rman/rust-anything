//! ESTree nodes as `crate::value::Value`, as oxc serializes them and Prettier reads them.

use crate::value::Value;

pub trait NodeExt {
    fn ty(&self) -> &str;
    fn is(&self, ty: &str) -> bool;
    fn field(&self, key: &str) -> Option<&Value>;
    fn list(&self, key: &str) -> &[Value];
    fn str_of(&self, key: &str) -> Option<&str>;
    fn flag(&self, key: &str) -> bool;
    fn start(&self) -> usize;
    fn end(&self) -> usize;
    fn parenthesized(&self) -> bool;
    fn same(&self, other: &Value) -> bool;
}

impl NodeExt for Value {
    fn ty(&self) -> &str {
        self.get("type").and_then(Value::as_str).unwrap_or("")
    }

    fn is(&self, ty: &str) -> bool {
        self.ty() == ty
    }

    fn field(&self, key: &str) -> Option<&Value> {
        self.get(key).filter(|v| !v.is_null())
    }

    fn list(&self, key: &str) -> &[Value] {
        self.get(key)
            .and_then(Value::as_array)
            .map_or(&[], Vec::as_slice)
    }

    fn str_of(&self, key: &str) -> Option<&str> {
        self.get(key).and_then(Value::as_str)
    }

    fn flag(&self, key: &str) -> bool {
        self.get(key).and_then(Value::as_bool).unwrap_or(false)
    }

    fn start(&self) -> usize {
        let start = self.get("start").and_then(Value::as_u64).unwrap_or(0) as usize;
        let decorators = self
            .field("declaration")
            .map(|d| d.list("decorators"))
            .filter(|d| !d.is_empty())
            .unwrap_or_else(|| self.list("decorators"));
        match decorators.first() {
            Some(first) => first.start().min(start),
            None => start,
        }
    }

    fn end(&self) -> usize {
        self.get("end").and_then(Value::as_u64).unwrap_or(0) as usize
    }

    fn parenthesized(&self) -> bool {
        self.get("extra")
            .and_then(|e| e.get("parenthesized"))
            .and_then(Value::as_bool)
            .unwrap_or(false)
    }

    fn same(&self, other: &Value) -> bool {
        std::ptr::eq(self, other)
    }
}

pub fn is_literal(n: &Value) -> bool {
    n.is("Literal")
}

pub fn is_string_literal(n: &Value) -> bool {
    n.is("Literal") && n.get("value").is_some_and(Value::is_string)
}

pub fn is_numeric_literal(n: &Value) -> bool {
    n.is("Literal") && n.get("value").is_some_and(Value::is_number) && n.field("bigint").is_none()
}

pub fn is_boolean_literal(n: &Value) -> bool {
    n.is("Literal") && n.get("value").is_some_and(Value::is_boolean)
}

pub fn is_regexp_literal(n: &Value) -> bool {
    n.is("Literal") && n.field("regex").is_some()
}

pub fn is_call_expression(n: &Value) -> bool {
    let n = skip_chain(n);
    matches!(n.ty(), "CallExpression" | "OptionalCallExpression")
}

pub fn is_member_expression(n: &Value) -> bool {
    let n = skip_chain(n);
    matches!(n.ty(), "MemberExpression" | "OptionalMemberExpression")
}

pub fn skip_chain(n: &Value) -> &Value {
    if n.is("ChainExpression") {
        n.field("expression").unwrap_or(n)
    } else {
        n
    }
}

/// `isObjectProperty`: object methods (`Property` with `method` or `kind` get/set) are not
/// properties here.
pub fn is_object_property(n: &Value) -> bool {
    n.is("ObjectProperty") || (n.is("Property") && !crate::estree::is_method(n))
}

pub fn is_function_or_arrow(n: &Value) -> bool {
    matches!(n.ty(), "FunctionExpression" | "ArrowFunctionExpression")
}

pub fn is_binaryish(n: &Value) -> bool {
    matches!(
        n.ty(),
        "BinaryExpression" | "LogicalExpression" | "NGPipeExpression"
    )
}

pub fn is_jsx_element(n: &Value) -> bool {
    matches!(n.ty(), "JSXElement" | "JSXFragment")
}

pub fn is_object_or_record(n: &Value) -> bool {
    matches!(n.ty(), "ObjectExpression" | "RecordExpression")
}

pub fn is_array_or_tuple(n: &Value) -> bool {
    matches!(n.ty(), "ArrayExpression" | "TupleExpression")
}

pub fn is_template_literal(n: &Value) -> bool {
    n.is("TemplateLiteral")
}
