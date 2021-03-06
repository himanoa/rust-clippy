use if_chain::if_chain;
use rustc_ast::ast::{FloatTy, LitKind};
use rustc_errors::Applicability;
use rustc_hir::def::Res;
use rustc_hir::{BinOpKind, Expr, ExprKind, Path, PathSegment, PrimTy, QPath, Ty, TyKind};
use rustc_lint::{LateContext, LateLintPass};
use rustc_session::{declare_lint_pass, declare_tool_lint};
use rustc_span::{source_map::Spanned, symbol::SymbolStr, Span};

use crate::consts::{constant, Constant};
use crate::utils::paths;
use crate::utils::{match_type, snippet_with_applicability, span_lint_and_sugg};

declare_clippy_lint! {
    /// **What it does:** Checks for calculation of subsecond microseconds or milliseconds
    /// from other `Duration` methods.
    ///
    /// **Why is this bad?** It's more concise to call `Duration::subsec_micros()` or
    /// `Duration::subsec_millis()` or `Duration::as_secs` than to calculate them.
    ///
    /// **Known problems:** None.
    ///
    /// **Example:**
    /// ```rust
    /// # use std::time::Duration;
    /// let dur = Duration::new(5, 0);
    ///
    /// // Bad
    /// let _micros = dur.subsec_nanos() / 1_000;
    /// let _millis = dur.subsec_nanos() / 1_000_000;
    /// let secs_f64 = diff.as_secs() as f64 + diff.subsec_milis() as f64 / 1_000.0;
    /// let secs_f64 = diff.as_secs() as f64 + diff.subsec_nanos() as f64 / 1_000_000_000.0;
    /// let secs_f64 = diff.as_secs() as f64 + diff.subsec_nanos() as f64 / 1_000_000_000.0;
    ///
    /// let secs_f32 = diff.as_secs() as f32 + diff.subsec_milis() as f32 / 1_000.0;
    /// let secs_f32 = diff.as_secs() as f32 + diff.subsec_nanos() as f32 / 1_000_000_000.0;
    /// let secs_f32 = diff.as_secs() as f32 + diff.subsec_nanos() as f32 / 1_000_000_000.0;
    ///
    /// // Good
    /// let _micros = dur.subsec_micros();
    /// let _millis = dur.subsec_millis();
    /// let secs_f64 = diff.as_secs_f64();
    /// let secs_f64 = diff.as_secs_f64();
    /// let secs_f64 = diff.as_secs_f64();
    ///
    /// let secs_f32 = diff.as_secs_f32();
    /// let secs_f32 = diff.as_secs_f32();
    /// let secs_f32 = diff.as_secs_f32();
    /// ```
    pub MANUAL_DURATION_CALCS,
    complexity,
    "checks for calculation of subsecond microseconds or milliseconds"
}

declare_lint_pass!(ManualDurationCalcs => [MANUAL_DURATION_CALCS]);

fn get_cast_type<'tcx>(ty: &'tcx Ty<'_>) -> Option<&'tcx PrimTy> {
    if let TyKind::Path(QPath::Resolved(
        _,
        Path {
            res: Res::PrimTy(pt), ..
        },
    )) = ty.kind
    {
        return Some(pt);
    }
    None
}

#[derive(Debug)]
struct ParsedMethodCallExpr<'tcx> {
    receiver: Span,
    method_name: SymbolStr,
    cast_type: Option<&'tcx PrimTy>,
}

impl ParsedMethodCallExpr<'tcx> {
    fn new(receiver: Span, method_name: SymbolStr, cast_type: Option<&'tcx PrimTy>) -> ParsedMethodCallExpr<'tcx> {
        ParsedMethodCallExpr {
            receiver,
            method_name,
            cast_type,
        }
    }
}

fn parse_method_call_expr<'tcx>(
    cx: &LateContext<'tcx>,
    value_expr: &'tcx Expr<'_>,
    cast_type: Option<&'tcx PrimTy>,
) -> Option<ParsedMethodCallExpr<'tcx>> {
    match value_expr.kind {
        ExprKind::Cast(expr, ty) => {
            let cast_type = get_cast_type(ty);
            parse_method_call_expr(cx, &expr, cast_type)
        },
        ExprKind::MethodCall(ref method_path, _, ref args, _) => {
            if match_type(cx, cx.typeck_results().expr_ty(&args[0]).peel_refs(), &paths::DURATION) {
                return Some(ParsedMethodCallExpr::new(
                    args[0].span,
                    method_path.ident.as_str(),
                    cast_type,
                ));
            }
            None
        },
        _ => None,
    }
}

#[derive(Debug)]
struct ParsedDivisionExpr<'tcx> {
    receiver: Span,
    method_name: SymbolStr,
    divisor: Constant,
    cast_type: Option<&'tcx PrimTy>,
}

impl ParsedDivisionExpr<'tcx> {
    fn new(
        receiver: Span,
        method_name: SymbolStr,
        divisor: Constant,
        cast_type: Option<&'tcx PrimTy>,
    ) -> ParsedDivisionExpr<'tcx> {
        ParsedDivisionExpr {
            receiver,
            method_name,
            divisor,
            cast_type,
        }
    }
}

fn parse_division_expr(cx: &LateContext<'tcx>, expr: &'tcx Expr<'_>) -> Option<ParsedDivisionExpr<'tcx>> {
    if_chain! {
        if let ExprKind::Binary(
            Spanned {
                node: BinOpKind::Div, ..
            },
            ref dividend_expr,
            ref divisor_expression,
        ) = expr.kind;
        if let Some(dividend) = parse_method_call_expr(cx, dividend_expr, None);
        if let Some((divisor, _)) = constant(cx, cx.typeck_results(), divisor_expression);
        then {
            Some(ParsedDivisionExpr::new(dividend.receiver, dividend.method_name, divisor, dividend.cast_type))
        } else {
            None
        }
    }
}

struct ParsedMultipleExpr {
    method_name: SymbolStr,
    multipilier: Constant,
}

impl ParsedMultipleExpr {
    fn new(method_name: SymbolStr, multipilier: Constant) -> ParsedMultipleExpr {
        ParsedMultipleExpr {
            method_name,
            multipilier,
        }
    }
}

fn extract_multiple_expr<'tcx>(cx: &LateContext<'tcx>, expr: &'tcx Expr<'_>) -> Option<ParsedMultipleExpr> {
    fn parse<'tcx>(
        cx: &LateContext<'tcx>,
        method_call_expr: &'tcx Expr<'_>,
        multiplier_expr: &'tcx Expr<'_>,
    ) -> Option<ParsedMultipleExpr> {
        if_chain! {
            if let Some(method_call) = parse_method_call_expr(cx, method_call_expr, None);
            if let Some((multipiler, _)) = constant(cx, cx.typeck_results(), multiplier_expr);
            then {
                Some(ParsedMultipleExpr::new(method_call.method_name, multipiler))
            } else {
                None
            }
        }
    }

    match expr.kind {
        ExprKind::Binary(
            Spanned {
                node: BinOpKind::Mul, ..
            },
            ref left,
            ref right,
        ) => Some((left, right)),
        _ => None,
    }
    .and_then(|splited_mul| {
        let patterns = [(splited_mul.0, splited_mul.1), (splited_mul.1, splited_mul.0)];
        patterns.iter().find_map(|expr| parse(cx, expr.0, expr.1))
    })
}

impl<'tcx> ManualDurationCalcs {
    pub fn duration_subsec(cx: &LateContext<'tcx>, expr: &'tcx Expr<'_>) {
        if_chain! {
            if let ExprKind::Binary(Spanned { node: BinOpKind::Div, .. }, ref left, ref right) = expr.kind;
            if let ExprKind::MethodCall(ref method_path, _ , ref args, _) = left.kind;
            if match_type(cx, cx.typeck_results().expr_ty(&args[0]).peel_refs(), &paths::DURATION);
            if let Some((Constant::Int(divisor), _)) = constant(cx, cx.typeck_results(), right);
            then {
                let suggested_fn = match (method_path.ident.as_str().as_ref(), divisor) {
                    ("subsec_micros", 1_000) | ("subsec_nanos", 1_000_000) => "subsec_millis",
                    ("subsec_nanos", 1_000) => "subsec_micros",
                    _ => return,
                };
                let mut applicability = Applicability::MachineApplicable;
                span_lint_and_sugg(
                    cx,
                    MANUAL_DURATION_CALCS,
                    expr.span,
                    &format!("calling `{}()` is more concise than this calculation", suggested_fn),
                    "try",
                    format!(
                        "{}.{}()",
                        snippet_with_applicability(cx, args[0].span, "_", &mut applicability),
                        suggested_fn
                        ),
                        applicability,
                        );
            }
        }
    }

    pub fn duration_as_secs(cx: &LateContext<'tcx>, expr: &'tcx Expr<'_>) {
        #[derive(Debug)]
        struct ParsedExpr<'tcx> {
            receiver: Span,
            add_method_name: String,
            cast_type: Option<&'tcx PrimTy>,
            multipilier_method_name: String,
            multipilier: Constant,
        }

        fn parse<'tcx>(
            cx: &LateContext<'tcx>,
            multiplier_expr: &'tcx Expr<'_>,
            method_call_expr: &'tcx Expr<'_>,
        ) -> Option<ParsedExpr<'tcx>> {
            if_chain! {
                if let Some(mul) = extract_multiple_expr(cx, multiplier_expr);
                if let Some(method_call) = parse_method_call_expr(cx, method_call_expr, None);
                then {
                    Some(ParsedExpr {
                        receiver: method_call.receiver,
                        add_method_name: method_call.method_name.to_string(),
                        cast_type: method_call.cast_type,
                        multipilier_method_name: mul.method_name.to_string(),
                        multipilier: mul.multipilier
                    })
                } else {
                    None
                }
            }
        }

        if_chain! {
            if let ExprKind::Binary(Spanned { node: BinOpKind::Div, .. }, left, right) = expr.kind;
            if let ExprKind::Binary(Spanned { node: BinOpKind::Add, .. }, add_left, add_right) = left.kind;
            if let Some((divisor, _)) = constant(cx, cx.typeck_results(), right);
            then {
                [(add_right, add_left), (add_left, add_right)]
                    .iter()
                    .flat_map(|e| parse(cx, e.0, e.1))
                    .for_each(|e| {
                        let suggested_fn = match (
                            e.multipilier_method_name.as_str(),
                            e.cast_type,
                            e.add_method_name.as_str(),
                            &divisor
                        ) {
                            ("as_secs", Some(PrimTy::Float(FloatTy::F64)), "subsec_millis", Constant::F64(divisor)) if (divisor - 1_000.0).abs() < f64::EPSILON => "as_secs_f64",
                            ("as_secs", Some(PrimTy::Float(FloatTy::F32)), "subsec_millis", Constant::F32(divisor)) if (divisor - 1_000.0).abs() < f32::EPSILON => "as_secs_f32",
                            _ => return
                        };

                        let mut applicability = Applicability::MachineApplicable;
                        span_lint_and_sugg(
                            cx,
                            MANUAL_DURATION_CALCS,
                            expr.span,
                            &format!("calling `{}()` is more concise than this calculation", suggested_fn),
                            "try",
                            format!(
                                "{}.{}()",
                                snippet_with_applicability(cx, e.receiver, "_", &mut applicability),
                                    suggested_fn
                                ),
                                applicability,
                        );
                    })
            }
        }
    }

    pub fn manual_re_implementation_lower_the_unit(cx: &LateContext<'tcx>, expr: &'tcx Expr<'_>) {
        fn parse<'tcx>(
            cx: &LateContext<'tcx>,
            multipilication_expr: &'tcx Expr<'_>,
            method_call_expr: &'tcx Expr<'_>,
            cast_type: Option<&'tcx PrimTy>,
        ) -> Option<(SymbolStr, Constant, SymbolStr, Option<&'tcx PrimTy>, Span)> {
            if_chain! {
                if let ExprKind::Cast(expr, ty) = method_call_expr.kind;
                if let Some(ct) = get_cast_type(ty);
                then {
                    return parse(cx, multipilication_expr, expr, Some(ct))
                }
            }

            if_chain! {
                if let ExprKind::MethodCall(ref method_path, _ , ref args, _) = method_call_expr.kind;
                if let Some(ParsedMultipleExpr {
                    method_name: mul_method_name, multipilier, ..
                }) = extract_multiple_expr(cx, multipilication_expr);
                if match_type(cx, cx.typeck_results().expr_ty(&args[0]).peel_refs(), &paths::DURATION);
                then  {
                    Some((mul_method_name, multipilier, method_path.ident.as_str(), cast_type, args[0].span))
                } else {
                    None
                }
            }
        }

        if let ExprKind::Binary(
            Spanned {
                node: BinOpKind::Add, ..
            },
            ref left,
            ref right,
        ) = expr.kind
        {
            [(left, right), (right, left)]
                .iter()
                .flat_map(|expr| parse(cx, expr.0, expr.1, None))
                .for_each(|r| {
                    let suggested_fn = match (r.0.to_string().as_str(), r.1, r.2.to_string().as_str()) {
                        ("as_secs", Constant::Int(1_000_000_000), "subsec_nanos") => "as_nanos",
                        ("as_secs", Constant::Int(1_000), "subsec_millis") => "as_millis",
                        ("as_secs", Constant::F64(multiplier), "subsec_millis")
                            if (multiplier - 1_000_000_000.0).abs() < f64::EPSILON =>
                        {
                            "as_millis"
                        },
                        ("as_secs", Constant::F32(multiplier), "subsec_millis")
                            if (multiplier - 1_000_000_000.0).abs() < f32::EPSILON =>
                        {
                            "as_millis"
                        },
                        ("as_secs", Constant::F64(multiplier), "subsec_millis")
                            if (multiplier - 1_000.0).abs() < f64::EPSILON =>
                        {
                            "as_millis"
                        },
                        ("as_secs", Constant::F32(multiplier), "subsec_millis")
                            if (multiplier - 1_000.0).abs() < f32::EPSILON =>
                        {
                            "as_millis"
                        },
                        _ => return,
                    };

                    let mut applicability = Applicability::MachineApplicable;

                    if let Some(cast_type) = r.3 {
                        span_lint_and_sugg(
                            cx,
                            MANUAL_DURATION_CALCS,
                            expr.span,
                            &format!("no manual re-implementationa of the {}", suggested_fn),
                            "try",
                            format!(
                                "{}.{}() as {}",
                                snippet_with_applicability(cx, r.4, "_", &mut applicability),
                                suggested_fn,
                                cast_type.name_str()
                            ),
                            applicability,
                        );
                    }
                });
        };
    }

    pub fn manual_re_implementation_upper_the_unit(cx: &LateContext<'tcx>, expr: &'tcx Expr<'_>) {
        if_chain! {
            if let ExprKind::Binary(
                Spanned {
                    node: BinOpKind::Add, ..
                },
                ref add_left_expr,
                ref rest,
            ) = expr.kind;
            if let Some(parsed_div) = parse_division_expr(cx, &rest);
            if let Some(parsed_add) = parse_method_call_expr(cx, &add_left_expr, None);
            then {
                let suggested_fn = match (
                    parsed_add.method_name.to_string().as_str(),
                    parsed_add.cast_type,
                    parsed_div.method_name.to_string().as_str(),
                    parsed_div.cast_type,
                    parsed_div.divisor
                ) {
                    ("as_secs", Some(PrimTy::Float(FloatTy::F64)), "subsec_millis", Some(PrimTy::Float(FloatTy::F64)), Constant::F64(divisor)) if (divisor - 1000.0).abs() < f64::EPSILON => {
                        "as_secs_f64"
                    },
                    ("as_secs", Some(PrimTy::Float(FloatTy::F64)), "subsec_nanos", Some(PrimTy::Float(FloatTy::F64)), Constant::F64(divisor)) if (divisor - 1_000_000_000.0).abs() < f64::EPSILON => {
                        "as_secs_f64"
                    },
                    ("as_secs", Some(PrimTy::Float(FloatTy::F32)), "subsec_millis", Some(PrimTy::Float(FloatTy::F32)), Constant::F32(divisor)) if (divisor - 1000.0).abs() < f32::EPSILON => {
                        "as_secs_f32"
                    },
                    ("as_secs", Some(PrimTy::Float(FloatTy::F32)), "subsec_nanos", Some(PrimTy::Float(FloatTy::F32)), Constant::F32(divisor)) if (divisor - 1_000_000_000.0).abs() < f32::EPSILON => {
                        "as_secs_f32"
                    },
                    _ => { return;  }
                };

                let mut applicability = Applicability::MachineApplicable;

                span_lint_and_sugg(
                    cx,
                    MANUAL_DURATION_CALCS,
                    expr.span,
                    &format!("no manual re-implementationa of the {}", suggested_fn),
                    "try",
                    format!(
                        "{}.{}()",
                        snippet_with_applicability(cx, parsed_add.receiver, "_", &mut applicability),
                        suggested_fn
                    ),
                    applicability,
                );
            }
        }
    }
}

impl<'tcx> LateLintPass<'tcx> for ManualDurationCalcs {
    fn check_expr(&mut self, cx: &LateContext<'tcx>, expr: &'tcx Expr<'_>) {
        ManualDurationCalcs::duration_as_secs(cx, expr);
        ManualDurationCalcs::manual_re_implementation_lower_the_unit(cx, expr);
        ManualDurationCalcs::manual_re_implementation_upper_the_unit(cx, expr);
        ManualDurationCalcs::duration_subsec(cx, expr);
    }
}
