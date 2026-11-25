//! Database equation strings (e.g. `combatformulas.dbr`), evaluated exactly
//! like the engine's shunting-yard parser (Game.dll `Equation::Parse`):
//! - tokens: numbers (`[0-9.]+`, C `atof`: [`gd_formats::util::atof`]), identifiers (`[A-Za-z]+`),
//!   `+ - * / ^ ( )`, spaces; anything else is a parse error;
//! - precedence `+ -` 0, `* /` 1, unary `-` 2, `^` 3; every binary operator
//!   is left-associative (`2^3^2` = 64); unary `-` binds looser than `^`
//!   (`-2^2` = -4); unary `+` is skipped;
//! - arithmetic is f32 (`powf` for `^`); unresolved variables read as 0.
//!
//! PORT: these semantics were read off `Equation::Parse` (a caller is the
//! destructible loader) and are pinned by the stock equations
//! the data tests evaluate (pth, regen, experience levels; see
//! `tests/sim/stats_db.rs`), but the parser's own decompile is not in the notes,
//! so the token set, precedence and associativity are not individually VERIFIED.
//! PORT: the engine builds an RPN list and evaluates it iteratively; the port
//! builds a tree evaluated recursively, so [`Expr::parse`] rejects trees deeper
//! than [`MAX_DEPTH`] (bad data: no stock equation is near it).
//!
//! A hot evaluator resolves its variable names to slots once at parse time
//! ([`Expr::parse_with`]) and evaluates against a value array
//! ([`Expr::eval_slots`]) instead of comparing names per evaluation.
use anyhow::{Result, bail};

#[derive(Debug, Clone, PartialEq)]
pub enum Expr {
    Num(f32),
    Var(String),
    /// A variable [`Expr::parse_with`] resolved: its slot in the values of
    /// [`Expr::eval_slots`], and its name for [`Expr::eval`].
    Slot(u8, String),
    Neg(Box<Expr>),
    Bin(Op, Box<Expr>, Box<Expr>),
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Op {
    Add,
    Sub,
    Mul,
    Div,
    Pow,
}

impl Op {
    /// `a <op> b` in f32 (`powf` for `^`).
    fn apply(self, a: f32, b: f32) -> f32 {
        match self {
            Op::Add => a + b,
            Op::Sub => a - b,
            Op::Mul => a * b,
            Op::Div => a / b,
            Op::Pow => a.powf(b),
        }
    }
}

#[derive(Clone, Copy)]
enum StackOp {
    Bin(Op),
    Neg,
    Paren,
}

impl StackOp {
    fn prec(self) -> u32 {
        match self {
            StackOp::Bin(Op::Add | Op::Sub) => 0,
            StackOp::Bin(Op::Mul | Op::Div) => 1,
            StackOp::Neg => 2,
            StackOp::Bin(Op::Pow) => 3,
            StackOp::Paren => 4,
        }
    }
    /// Whether this operator (on the stack) is reduced before pushing `new`.
    fn pops_before(self, new: StackOp) -> bool {
        match self {
            StackOp::Paren => false,
            StackOp::Neg => self.prec() > new.prec(),
            StackOp::Bin(_) => self.prec() >= new.prec(),
        }
    }
}

/// Deepest tree [`Expr::parse`] accepts (bad data: `eval` and drop recurse
/// per level; stock equations nest a handful of levels).
pub const MAX_DEPTH: u32 = 256;

/// Operand stack entries: a subtree and its depth.
fn push(out: &mut Vec<(Expr, u32)>, e: Expr, depth: u32) -> Result<()> {
    if depth > MAX_DEPTH {
        bail!("equation nested deeper than {MAX_DEPTH}");
    }
    out.push((e, depth));
    Ok(())
}

fn reduce(op: StackOp, out: &mut Vec<(Expr, u32)>) -> Result<()> {
    match op {
        StackOp::Neg => {
            let (a, d) = out
                .pop()
                .ok_or_else(|| anyhow::anyhow!("missing operand"))?;
            push(out, Expr::Neg(Box::new(a)), d + 1)?;
        }
        StackOp::Bin(o) => {
            let (Some((b, db)), Some((a, da))) = (out.pop(), out.pop()) else {
                bail!("missing operand")
            };
            push(out, Expr::Bin(o, Box::new(a), Box::new(b)), da.max(db) + 1)?;
        }
        StackOp::Paren => bail!("unbalanced '('"),
    }
    Ok(())
}

impl Expr {
    pub fn parse(src: &str) -> Result<Expr> {
        Self::parse_with(src, &|_| None)
    }

    /// [`Expr::parse`] with the variables `slot` knows resolved to their slot.
    pub fn parse_with(src: &str, slot: &dyn Fn(&str) -> Option<u8>) -> Result<Expr> {
        let b = src.as_bytes();
        let mut out: Vec<(Expr, u32)> = Vec::new();
        let mut ops: Vec<StackOp> = Vec::new();
        let mut expect_operand = true;
        let mut i = 0;
        while i < b.len() {
            let c = b[i];
            if c.is_ascii_digit() || c == b'.' {
                let s = i;
                while i < b.len() && (b[i].is_ascii_digit() || b[i] == b'.') {
                    i += 1;
                }
                out.push((Expr::Num(gd_formats::util::atof(&src[s..i]) as f32), 1));
                expect_operand = false;
                continue;
            }
            if c.is_ascii_alphabetic() {
                let s = i;
                while i < b.len() && b[i].is_ascii_alphabetic() {
                    i += 1;
                }
                let name = &src[s..i];
                let e = match slot(name) {
                    Some(k) => Expr::Slot(k, name.to_owned()),
                    None => Expr::Var(name.to_owned()),
                };
                out.push((e, 1));
                expect_operand = false;
                continue;
            }
            match c {
                b' ' => {}
                b'(' => {
                    ops.push(StackOp::Paren);
                    expect_operand = true;
                }
                b')' => loop {
                    match ops.pop() {
                        Some(StackOp::Paren) => break,
                        Some(op) => reduce(op, &mut out)?,
                        None => bail!("unbalanced ')' in {src:?}"),
                    }
                },
                b'+' | b'-' | b'*' | b'/' | b'^' => {
                    let new = if expect_operand {
                        match c {
                            b'+' => {
                                i += 1;
                                continue;
                            }
                            b'-' => StackOp::Neg,
                            _ => bail!("operator {:?} without left operand in {src:?}", c as char),
                        }
                    } else {
                        StackOp::Bin(match c {
                            b'+' => Op::Add,
                            b'-' => Op::Sub,
                            b'*' => Op::Mul,
                            b'/' => Op::Div,
                            _ => Op::Pow,
                        })
                    };
                    // A prefix operator never reduces what precedes it.
                    if matches!(new, StackOp::Bin(_)) {
                        while let Some(&top) = ops.last() {
                            if !top.pops_before(new) {
                                break;
                            }
                            ops.pop();
                            reduce(top, &mut out)?;
                        }
                    }
                    ops.push(new);
                    expect_operand = true;
                }
                _ => bail!("unexpected {:?} in {src:?}", c as char),
            }
            i += 1;
        }
        while let Some(op) = ops.pop() {
            reduce(op, &mut out)?;
        }
        if out.len() != 1 {
            bail!("malformed equation {src:?}");
        }
        Ok(out.pop().unwrap().0)
    }

    pub fn eval(&self, vars: &dyn Fn(&str) -> Option<f32>) -> f32 {
        match self {
            Expr::Num(n) => *n,
            Expr::Var(v) | Expr::Slot(_, v) => vars(v).unwrap_or(0.0),
            Expr::Neg(a) => -a.eval(vars),
            Expr::Bin(op, a, b) => op.apply(a.eval(vars), b.eval(vars)),
        }
    }

    /// Evaluates with slot variables read from `values` (0 past its end) and
    /// the rest resolved by `slot` (an expression parsed without it).
    pub fn eval_slots(&self, values: &[f32], slot: &dyn Fn(&str) -> Option<u8>) -> f32 {
        let get = |k: u8| values.get(usize::from(k)).copied().unwrap_or(0.0);
        match self {
            Expr::Num(n) => *n,
            Expr::Slot(k, _) => get(*k),
            Expr::Var(v) => slot(v).map_or(0.0, get),
            Expr::Neg(a) => -a.eval_slots(values, slot),
            Expr::Bin(op, a, b) => op.apply(a.eval_slots(values, slot), b.eval_slots(values, slot)),
        }
    }
}

/// A one-variable record equation (`skillLevelN`, `itemSkillLevelEq`,
/// creation costs): [`Expr::parse`] + [`Expr::eval`] with `var` = `value`
/// (name matched ignoring ASCII case; other variables read as 0).
/// `None` on a parse error.
pub fn eval_var(eq: &str, var: &str, value: f32) -> Option<f32> {
    Expr::parse(eq)
        .ok()
        .map(|e| e.eval(&|n| n.eq_ignore_ascii_case(var).then_some(value)))
}

#[cfg(test)]
mod tests {
    use super::*;
    fn ev(s: &str) -> f32 {
        Expr::parse(s).unwrap().eval(&|v| match v {
            "x" => Some(3.0),
            _ => None,
        })
    }
    #[test]
    fn precedence() {
        assert_eq!(ev("1+2*3"), 7.0);
        assert_eq!(ev("2^3^2"), 64.0);
        assert_eq!(ev("-2^2"), -4.0);
        assert_eq!(ev("-x*2"), -6.0);
        assert_eq!(ev("10-4-3"), 3.0);
        assert_eq!(ev("8/4/2"), 1.0);
        assert_eq!(ev("(1+2)*(x - 1)"), 6.0);
        assert_eq!(ev("unknown+1"), 1.0);
        assert_eq!(ev("1.5.2"), 1.5);
    }
}
