use sylt_common::error::Error;

use crate::{Assignable, Context, Expression, Identifier, Next, ParseResult, Prec, T, Type, assignable, block_statement, expect, parse_type, raise_syntax_error, syntax_error};

/// Parse an [ExpressionKind::Function]: `fn a: int, b: bool -> bool <statement>`
fn function<'t>(ctx: Context<'t>) -> ParseResult<'t, Expression> {
    let span = ctx.span();
    let mut ctx = expect!(ctx, T::Fn, "Expected 'fn' for function expression");
    let mut params = Vec::new();
    // Parameters
    let ret = loop {
        match ctx.token() {
            T::Identifier(name) => {
                // Parameter name
                let ident = Identifier {
                    span: ctx.span(),
                    name: name.clone(),
                };
                ctx = expect!(ctx.skip(1), T::Colon, "Expected ':' after parameter name");
                // Parameter type
                let (_ctx, param) = parse_type(ctx)?;
                ctx = _ctx; // assign to outer

                params.push((ident, param));

                ctx = if matches!(ctx.token(), T::Comma | T::Arrow | T::LeftBrace) {
                    ctx.skip_if(T::Comma)
                } else {
                    raise_syntax_error!(ctx, "Expected ',' '{{' or '->' after type parameter")
                };
            }

            // Parse return type
            T::Arrow => {
                ctx = ctx.skip(1);
                break if let Ok((_ctx, ret)) = parse_type(ctx) {
                    ctx = _ctx; // assign to outer
                    ret
                } else {
                    use crate::RuntimeType::Void;
                    use crate::TypeKind::Resolved;
                    Type {
                        // If we couldn't parse the return type, we assume `-> Void`.
                        span: ctx.span(),
                        kind: Resolved(Void),
                    }
                };
            }

            T::LeftBrace => {
                use crate::RuntimeType::Void;
                use crate::TypeKind::Resolved;
                // No return type so we assume `-> Void`.
                break Type {
                    span: ctx.span(),
                    kind: Resolved(Void),
                };
            }

            t => {
                raise_syntax_error!(ctx, "Didn't expect '{:?}' in function", t);
            }
        }
    };

    use crate::ExpressionKind::Function;
    // Parse the function statement.
    let (ctx, statement) = block_statement(ctx)?;
    let function = Function {
        name: "lambda".into(),
        params,
        ret,
        body: Box::new(statement),
    };

    Ok((
        ctx,
        Expression {
            span,
            kind: function,
        },
    ))
}

/// Parse an expression until we reach a token with higher precedence.
fn parse_precedence<'t>(ctx: Context<'t>, prec: Prec) -> ParseResult<'t, Expression> {
    // Initial value, e.g. a number value, assignable, ...
    let (mut ctx, mut expr) = prefix(ctx)?;
    while prec <= precedence(ctx.token()) {
        if let Ok((_ctx, _expr)) = infix(ctx, &expr) {
            // assign to outer
            ctx = _ctx;
            expr = _expr;
        } else {
            break;
        }
    }
    Ok((ctx, expr))
}

/// Return a [Token]'s precedence.
///
/// See the documentation on [Prec] for how to interpret and compare the
/// variants.
#[rustfmt::skip]
fn precedence(token: &T) -> Prec {
    match token {
        T::LeftBracket => Prec::Index,

        T::Star | T::Slash => Prec::Factor,

        T::Minus | T::Plus => Prec::Term,

        T::EqualEqual
        | T::Greater
        | T::GreaterEqual
        | T::Less
        | T::LessEqual
        | T::NotEqual => Prec::Comp,

        T::And => Prec::BoolAnd,
        T::Or => Prec::BoolOr,

        T::In => Prec::Index,

        T::AssertEqual => Prec::Assert,

        T::Arrow => Prec::Arrow,

        _ => Prec::No,
    }
}

/// Parse a single (primitive) value.
fn value<'t>(ctx: Context<'t>) -> Result<(Context<'t>, Expression), (Context<'t>, Vec<Error>)> {
    use crate::ExpressionKind::*;
    let (token, span, ctx) = ctx.eat();
    let kind = match token.clone() {
        T::Float(f) => Float(f),
        T::Int(i) => Int(i),
        T::Bool(b) => Bool(b),
        T::Nil => Nil,
        T::String(s) => Str(s),
        t => {
            raise_syntax_error!(ctx, "Cannot parse value, '{:?}' is not a valid value", t);
        }
    };
    Ok((ctx, Expression { span, kind }))
}

/// Parse something that begins at the start of an expression.
fn prefix<'t>(ctx: Context<'t>) -> ParseResult<'t, Expression> {
    use crate::ExpressionKind::Get;

    match ctx.token() {
        T::LeftParen => grouping_or_tuple(ctx),
        T::LeftBracket => list(ctx),
        T::LeftBrace => set_or_dict(ctx),

        T::Float(_) | T::Int(_) | T::Bool(_) | T::String(_) | T::Nil => value(ctx),
        T::Minus | T::Bang => unary(ctx),

        T::Identifier(_) => {
            // Blob initializations are expressions.
            if let Ok(result) = blob(ctx) {
                Ok(result)
            } else {
                let span = ctx.span();
                let (ctx, assign) = assignable(ctx)?;
                Ok((
                    ctx,
                    Expression {
                        span,
                        kind: Get(assign),
                    },
                ))
            }
        }

        t => {
            raise_syntax_error!(ctx, "No valid expression starts with '{:?}'", t);
        }
    }
}

/// Parse a unary operator followed by an expression, e.g. `-5`.
fn unary<'t>(ctx: Context<'t>) -> ParseResult<'t, Expression> {
    use crate::ExpressionKind::{Neg, Not};

    let (op, span, ctx) = ctx.eat();
    let (ctx, expr) = parse_precedence(ctx, Prec::Factor)?;
    let expr = Box::new(expr);

    let kind = match op {
        T::Minus => Neg(expr),
        T::Bang => Not(expr),

        _ => {
            raise_syntax_error!(ctx, "Invalid unary operator");
        }
    };
    Ok((ctx, Expression { span, kind }))
}

/// Parse an expression starting from an infix operator. Called by `parse_precedence`.
fn infix<'t>(ctx: Context<'t>, lhs: &Expression) -> ParseResult<'t, Expression> {
    use crate::ExpressionKind::*;

    // Parse an operator and a following expression
    // until we reach a token with higher precedence.
    let (op, span, ctx) = ctx.eat();
    let (ctx, rhs) = parse_precedence(ctx, precedence(op).next())?;

    // Left and right of the operator.
    let lhs = Box::new(lhs.clone());
    let rhs = Box::new(rhs);

    // Which expression kind to omit depends on the token.
    let kind = match op {
        // Simple arithmetic.
        T::Plus => Add(lhs, rhs),
        T::Minus => Sub(lhs, rhs),
        T::Star => Mul(lhs, rhs),
        T::Slash => Div(lhs, rhs),
        T::EqualEqual => Eq(lhs, rhs),
        T::NotEqual => Neq(lhs, rhs),
        T::Greater => Gt(lhs, rhs),
        T::GreaterEqual => Gteq(lhs, rhs),
        T::Less => Lt(lhs, rhs),
        T::LessEqual => Lteq(lhs, rhs),

        // Boolean operators.
        T::And => And(lhs, rhs),
        T::Or => Or(lhs, rhs),

        T::AssertEqual => AssertEq(lhs, rhs),

        T::In => In(lhs, rhs),

        // The cool arrow syntax. For example: `a->b(2)` compiles to `b(a, 2)`.
        T::Arrow => {
            use crate::AssignableKind::Call;
            // Rhs has to be an ExpressionKind::Get(AssignableKind::Call).
            if let Get(Assignable { kind: Call(callee, mut args), ..  }) = rhs.kind {
                // Insert lhs as the first argument.
                args.insert(0, *lhs);
                // Return the new expression.
                Get(Assignable {
                    kind: Call(callee, args),
                    span: rhs.span,
                })
            } else {
                raise_syntax_error!(ctx, "Expected a call-expression after '->'");
            }
        }

        // Unknown infix operator.
        _ => {
            return Err((ctx, Vec::new()));
        }
    };

    Ok((ctx, Expression { span, kind }))
}

/// Parse either a grouping parenthesis or a tuple.
///
/// Essentially, one-element tuples are groupings unless they end with a
/// comma. So `(1)` is parsed as the value `1` while `(1,)` is parsed as the
/// one-sized tuple containing `1`.
///
/// `()` as well as `(,)` are parsed as zero-sized tuples.
fn grouping_or_tuple<'t>(ctx: Context<'t>) -> ParseResult<'t, Expression> {
    let span = ctx.span();
    let mut ctx = expect!(ctx, T::LeftParen, "Expected '('");

    // The expressions contained in the parenthesis.
    let mut exprs = Vec::new();

    let mut is_tuple = matches!(ctx.token(), T::Comma | T::RightParen);
    loop {
        // Any initial comma is skipped since we checked it before entering the loop.
        ctx = ctx.skip_if(T::Comma);
        match ctx.token() {
            // Done.
            T::EOF | T::RightParen => {
                break;
            }

            // Another inner expression.
            _ => {
                let (_ctx, expr) = expression(ctx)?;
                exprs.push(expr);
                ctx = _ctx; // assign to outer
                // Not a tuple, until it is.
                is_tuple |= matches!(ctx.token(), T::Comma);
            }
        }
    }

    ctx = expect!(ctx, T::RightParen, "Expected ')'");

    use crate::ExpressionKind::Tuple;
    let result = if is_tuple {
        Expression {
            span,
            kind: Tuple(exprs),
        }
    } else {
        exprs.remove(0)
    };
    Ok((ctx, result))
}

/// Parse a blob instantiation, e.g. `A { b: 55 }`.
fn blob<'t>(ctx: Context<'t>) -> ParseResult<'t, Expression> {
    let span = ctx.span();
    let (ctx, blob) = assignable(ctx)?;
    let mut ctx = expect!(ctx, T::LeftBrace, "Expected '{{' after blob name");

    // The blob's fields.
    let mut fields = Vec::new();
    loop {
        match ctx.token() {
            T::Newline => {
                ctx = ctx.skip(1);
            }

            // Done with fields.
            T::RightBrace | T::EOF => {
                break;
            }

            // Another field, e.g. `b: 55`.
            T::Identifier(name) => {
                // Get the field name.
                let name = name.clone();

                ctx = expect!(ctx.skip(1), T::Colon, "Expected ':' after field name");
                // Get the value; `55` in the example above.
                let (_ctx, expr) = expression(ctx)?;
                ctx = _ctx; // assign to outer

                if !matches!(ctx.token(), T::Comma | T::Newline | T::RightBrace) {
                    raise_syntax_error!(ctx, "Expected a delimiter: newline or ','");
                }
                ctx = ctx.skip_if(T::Comma);

                fields.push((name, expr));
            }

            t => {
                raise_syntax_error!(ctx, "Unexpected token ('{:?}') in blob initalizer", t);
            }
        }
    }
    let ctx = expect!(ctx, T::RightBrace, "Expected '}}' after blob initalizer");

    if matches!(ctx.token(), T::Else) {
        raise_syntax_error!(ctx, "Parsed a blob instance not an if-statement");
    }

    use crate::ExpressionKind::Instance;
    Ok((
        ctx,
        Expression {
            span,
            kind: Instance { blob, fields },
        },
    ))
}

// Parse a list expression, e.g. `[1, 2, a(3)]`
fn list<'t>(ctx: Context<'t>) -> ParseResult<'t, Expression> {
    let span = ctx.span();
    let mut ctx = expect!(ctx, T::LeftBracket, "Expected '['");

    // `l := [\n1` is valid
    ctx = ctx.skip_while(T::Newline);

    // Inner experssions.
    let mut exprs = Vec::new();
    loop {
        match ctx.token() {
            // Done with inner expressions.
            T::EOF | T::RightBracket => {
                break;
            }

            // Another one.
            _ => {
                let (_ctx, expr) = expression(ctx)?;
                exprs.push(expr);
                ctx = _ctx; // assign to outer
                ctx = ctx.skip_if(T::Comma);
                ctx = ctx.skip_while(T::Newline); // newlines after expression is valid inside lists
            }
        }
    }

    ctx = expect!(ctx, T::RightBracket, "Expected ']'");
    use crate::ExpressionKind::List;
    Ok((
        ctx,
        Expression {
            span,
            kind: List(exprs),
        },
    ))
}

/// Parse either a set or dict expression.
///
/// `{:}` is parsed as the empty dict and {} is parsed as the empty set.
fn set_or_dict<'t>(ctx: Context<'t>) -> ParseResult<'t, Expression> {
    let span = ctx.span();
    let mut ctx = expect!(ctx, T::LeftBrace, "Expected '{{'");

    // The inner values of the set or dict.
    let mut exprs = Vec::new();
    // None => we don't know. Some(b) => we know b.
    let mut is_dict = None;
    loop {
        match ctx.token() {
            // Done.
            T::EOF | T::RightBrace => {
                break;
            }

            // Free-standing colon, i.e. "empty dict pair".
            T::Colon => {
                // Only valid if we don't know yet.
                if let Some(is_dict) = is_dict {
                    raise_syntax_error!(
                        ctx,
                        "Empty dict pair is invalid in a {}",
                        if is_dict { "dict" } else { "set" }
                    );
                }
                is_dict = Some(true);
                ctx = ctx.skip(1);
            }

            // Something that's part of an inner expression.
            _ => {
                // Parse the expression.
                let (_ctx, expr) = expression(ctx)?;
                ctx = _ctx; // assign to outer
                exprs.push(expr);

                // If a) we know we're a dict or b) the next token is a colon, parse the value of the dict.
                // Also, if we didn't know previously, store whether we're a dict or not.
                if *is_dict.get_or_insert_with(|| matches!(ctx.token(), T::Colon)) {
                    ctx = expect!(ctx, T::Colon, "Expected ':' for dict pair");
                    // Parse value expression.
                    let (_ctx, expr) = expression(ctx)?;
                    ctx = _ctx; // assign to outer
                    exprs.push(expr);
                }

                ctx = ctx.skip_if(T::Comma);
            }
        }
    }

    ctx = expect!(ctx, T::RightBrace, "Expected '}}'");

    use crate::ExpressionKind::{Dict, Set};
    // If we still don't know, assume we're a set.
    let kind = if is_dict.unwrap_or(false) {
        Dict(exprs)
    } else {
        Set(exprs)
    };

    Ok((ctx, Expression { span, kind }))
}

/// Parse a single expression.
///
/// An expression is either a function expression or a "normal"
/// expression that follows precedence rules.

pub fn expression<'t>(ctx: Context<'t>) -> ParseResult<'t, Expression> {
    match ctx.token() {
        T::Fn => function(ctx),
        _ => parse_precedence(ctx, Prec::No),
    }
}
