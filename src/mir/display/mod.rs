use crate::parse_tree::expr::BinaryOperator;

pub mod graphviz;
pub mod writer;

fn fmt_binop(op: &BinaryOperator) -> &'static str {
    match op {
        BinaryOperator::Plus => "+",
        BinaryOperator::Minus => "-",
        BinaryOperator::Times => "*",
        BinaryOperator::Div => "/",
        BinaryOperator::Modulo => "%",
        BinaryOperator::Eq => "==",
        BinaryOperator::Diff => "!=",
        BinaryOperator::Lt => "<",
        BinaryOperator::Leq => "<=",
        BinaryOperator::Gt => ">",
        BinaryOperator::Geq => ">=",
        BinaryOperator::And => "&&",
        BinaryOperator::Or => "||",
        BinaryOperator::BitAnd => "&",
        BinaryOperator::BitOr => "|",
        BinaryOperator::BitXor => "^",
    }
}
