// run-rustfix
// edition:2018

#![warn(clippy::use_self)]
#![allow(dead_code)]
#![allow(clippy::should_implement_trait)]

fn main() {}

mod use_self {
    struct Foo {}

    impl Foo {
        fn new() -> Foo {
            Foo {}
        }
        fn test() -> Foo {
            // FIXME: applicable here
            Foo::new()
        }
    }

    impl Default for Foo {
        // FIXME: applicable here
        fn default() -> Foo {
            // FIXME: applicable here
            Foo::new()
        }
    }
}

mod better {
    struct Foo {}

    impl Foo {
        fn new() -> Self {
            Self {}
        }
        fn test() -> Self {
            Self::new()
        }
    }

    impl Default for Foo {
        fn default() -> Self {
            Self::new()
        }
    }
}

mod lifetimes {
    struct Foo<'a> {
        foo_str: &'a str,
    }

    impl<'a> Foo<'a> {
        // Cannot use `Self` as return type, because the function is actually `fn foo<'b>(s: &'b str) ->
        // Foo<'b>`
        fn foo(s: &str) -> Foo {
            Foo { foo_str: s }
        }
        // cannot replace with `Self`, because that's `Foo<'a>`
        fn bar() -> Foo<'static> {
            Foo { foo_str: "foo" }
        }

        // FIXME: the lint does not handle lifetimed struct
        // `Self` should be applicable here
        fn clone(&self) -> Foo<'a> {
            Foo { foo_str: self.foo_str }
        }
    }
}

mod issue2894 {
    trait IntoBytes {
        fn into_bytes(&self) -> Vec<u8>;
    }

    // This should not be linted
    impl IntoBytes for u8 {
        fn into_bytes(&self) -> Vec<u8> {
            vec![*self]
        }
    }
}

mod existential {
    struct Foo;

    impl Foo {
        // FIXME:
        // TyKind::Def (used for `impl Trait` types) does not include type parameters yet.
        // See documentation in rustc_hir::hir::TyKind.
        // The hir tree walk stops at `impl Iterator` level and does not inspect &Foo.
        fn bad(foos: &[Foo]) -> impl Iterator<Item = &Foo> {
            foos.iter()
        }

        fn good(foos: &[Self]) -> impl Iterator<Item = &Self> {
            foos.iter()
        }
    }
}

mod tuple_structs {
    pub struct TS(i32);

    impl TS {
        pub fn ts() -> Self {
            TS(0)
        }
    }
}

mod macros {
    macro_rules! use_self_expand {
        () => {
            fn new() -> Foo {
                Foo {}
            }
        };
    }

    struct Foo {}

    impl Foo {
        use_self_expand!(); // Should lint in local macros
    }
}

mod nesting {
    struct Foo {}
    impl Foo {
        fn foo() {
            #[allow(unused_imports)]
            use self::Foo; // Can't use Self here
            struct Bar {
                foo: Foo, // Foo != Self
            }

            impl Bar {
                fn bar() -> Bar {
                    Bar { foo: Foo {} }
                }
            }

            // Can't use Self here
            fn baz() -> Foo {
                Foo {}
            }
        }

        // Should lint here
        fn baz() -> Foo {
            Foo {}
        }
    }

    enum Enum {
        A,
        B(u64),
        C { field: bool },
    }
    impl Enum {
        fn method() {
            #[allow(unused_imports)]
            use self::Enum::*; // Issue 3425
            static STATIC: Enum = Enum::A; // Can't use Self as type
        }

        fn method2() {
            let _ = Enum::B(42);
            let _ = Enum::C { field: true };
            let _ = Enum::A;
        }
    }
}

mod issue3410 {

    struct A;
    struct B;

    trait Trait<T> {
        fn a(v: T) -> Self;
    }

    impl Trait<Vec<A>> for Vec<B> {
        fn a(_: Vec<A>) -> Self {
            unimplemented!()
        }
    }

    impl<T> Trait<Vec<A>> for Vec<T>
    where
        T: Trait<B>,
    {
        fn a(v: Vec<A>) -> Self {
            <Vec<B>>::a(v).into_iter().map(Trait::a).collect()
        }
    }
}

#[allow(clippy::no_effect, path_statements)]
mod rustfix {
    mod nested {
        pub struct A {}
    }

    impl nested::A {
        const A: bool = true;

        fn fun_1() {}

        fn fun_2() {
            // FIXME: applicable here
            nested::A::fun_1();
            // FIXME: applicable here
            nested::A::A;

            nested::A {};
        }
    }
}

mod issue3567 {
    struct TestStruct {}
    impl TestStruct {
        fn from_something() -> Self {
            Self {}
        }
    }

    trait Test {
        fn test() -> TestStruct;
    }

    impl Test for TestStruct {
        fn test() -> TestStruct {
            // FIXME: applicable here
            TestStruct::from_something()
        }
    }
}

mod paths_created_by_lowering {
    use std::ops::Range;

    struct S {}

    impl S {
        const A: usize = 0;
        const B: usize = 1;

        // FIXME: applicable here
        async fn g() -> S {
            S {}
        }

        fn f<'a>(&self, p: &'a [u8]) -> &'a [u8] {
            // FIXME: applicable here twice
            &p[S::A..S::B]
        }
    }

    trait T {
        fn f<'a>(&self, p: &'a [u8]) -> &'a [u8];
    }

    impl T for Range<u8> {
        fn f<'a>(&self, p: &'a [u8]) -> &'a [u8] {
            &p[0..1]
        }
    }
}

// reused from #1997
mod generics {
    struct Foo<T> {
        value: T,
    }

    impl<T> Foo<T> {
        // `Self` is applicable here
        fn foo(value: T) -> Foo<T> {
            Foo { value }
        }

        // `Cannot` use `Self` as a return type as the generic types are different
        fn bar(value: i32) -> Foo<i32> {
            Foo { value }
        }
    }
}

mod issue4140 {
    pub struct Error<From, To> {
        _from: From,
        _too: To,
    }

    pub trait From<T> {
        type From;
        type To;

        fn from(value: T) -> Self;
    }

    pub trait TryFrom<T>
    where
        Self: Sized,
    {
        type From;
        type To;

        fn try_from(value: T) -> Result<Self, Error<Self::From, Self::To>>;
    }

    impl<F, T> TryFrom<F> for T
    where
        T: From<F>,
    {
        type From = T::From;
        type To = T::To;

        fn try_from(value: F) -> Result<Self, Error<Self::From, Self::To>> {
            Ok(From::from(value))
        }
    }

    impl From<bool> for i64 {
        type From = bool;
        type To = Self;

        fn from(value: bool) -> Self {
            if value {
                100
            } else {
                0
            }
        }
    }
}

mod issue2843 {
    trait Foo {
        type Bar;
    }

    impl Foo for usize {
        type Bar = u8;
    }

    impl<T: Foo> Foo for Option<T> {
        type Bar = Option<T::Bar>;
    }
}

mod issue3859 {
    pub struct Foo;
    pub struct Bar([usize; 3]);

    impl Foo {
        pub const BAR: usize = 3;

        pub fn foo() {
            const _X: usize = Foo::BAR;
            // const _Y: usize = Self::BAR;
        }
    }
}

mod issue4305 {
    trait Foo: 'static {}

    struct Bar;

    impl Foo for Bar {}

    impl<T: Foo> From<T> for Box<dyn Foo> {
        fn from(t: T) -> Self {
            // FIXME: applicable here
            Box::new(t)
        }
    }
}

mod lint_at_item_level {
    struct Foo {}

    #[allow(clippy::use_self)]
    impl Foo {
        fn new() -> Foo {
            Foo {}
        }
    }

    #[allow(clippy::use_self)]
    impl Default for Foo {
        fn default() -> Foo {
            Foo::new()
        }
    }
}

mod lint_at_impl_item_level {
    struct Foo {}

    impl Foo {
        #[allow(clippy::use_self)]
        fn new() -> Foo {
            Foo {}
        }
    }

    impl Default for Foo {
        #[allow(clippy::use_self)]
        fn default() -> Foo {
            Foo::new()
        }
    }
}

mod issue4734 {
    #[repr(C, packed)]
    pub struct X {
        pub x: u32,
    }

    impl From<X> for u32 {
        fn from(c: X) -> Self {
            unsafe { core::mem::transmute(c) }
        }
    }
}

mod nested_paths {
    use std::convert::Into;
    mod submod {
        pub struct B {}
        pub struct C {}

        impl Into<C> for B {
            fn into(self) -> C {
                C {}
            }
        }
    }

    struct A<T> {
        t: T,
    }

    impl<T> A<T> {
        fn new<V: Into<T>>(v: V) -> Self {
            Self { t: Into::into(v) }
        }
    }

    impl A<submod::C> {
        fn test() -> Self {
            // FIXME: applicable here
            A::new::<submod::B>(submod::B {})
        }
    }
}
