#![feature(const_fn, const_let)]
#![allow(unused_must_use)]

// Try to make CTFE actually do a lot of computation, without producing a big result.
// And without support for loops.

macro_rules! const_repeat {
    // Base case: Use 16 at the end to avoid function calls at the leaves as much as possibele.
    ([16] $e: expr, $T: ty) => {{
        $e; $e; $e; $e;
        $e; $e; $e; $e;
        $e; $e; $e; $e;
        $e; $e; $e; $e
    }};
    ([1] $e: expr, $T: ty) => {{
        $e
    }};
    // Recursive case: Take a 16
    ([16 $($n: tt)*] $e: expr, $T: ty) => {{
        const fn e() -> $T { const_repeat!([$($n)*] $e, $T) }
        e(); e(); e(); e();
        e(); e(); e(); e();
        e(); e(); e(); e();
        e(); e(); e(); e()
    }};
    // Recursive case: Take a 4
    ([4 $($n: tt)*] $e: expr, $T: ty) => {{
        const fn e() -> $T { const_repeat!([$($n)*] $e, $T) }
        e(); e(); e(); e()
    }};
    // Recursive case: Take a 2
    ([2 $($n: tt)*] $e: expr, $T: ty) => {{
        const fn e() -> $T { const_repeat!([$($n)*] $e, $T) }
        e(); e()
    }};
}
macro_rules! expensive_static {
    ($name: ident : $T: ty = $e : expr) =>
        (pub static $name : $T = const_repeat!([2 16 16 16 16 16] $e, $T);)
}

const fn inc(i: i32) -> i32 { i + 1 }

expensive_static!(CONST_FN_SIMPLE: i32 = inc(42));
