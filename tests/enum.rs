/// Todo:
///
/// - Compile-fail test with `Unknown` parameter, assert the default catch-all variant does not exist!
/// - Compile-fail: enum with fields
#[enum_ffi_newtype::enum_ffi(rust_enum_name = "FooRs")]
#[repr(u32)]
#[derive(Debug, PartialEq)]
enum Foo {
    Variant,
    Variant2,
    Variant3
}

#[enum_ffi_newtype::enum_ffi(non_zero)]
#[repr(u32)]
#[derive(Debug, PartialEq)]
enum FooNonZero {
    Variant = 1,
    Variant2,
    Variant3
}

#[enum_ffi_newtype::enum_ffi(catch_all = "Unknown")]
#[repr(u32)]
#[derive(Debug, PartialEq)]
enum FooWithCatchAll {
    Variant,
    Variant2,
    Variant3,
    Unknown
}

#[test]
fn test_roundtrip() {
    let foo: FooRs = Foo::Variant.into();
    let back: Foo = foo.into();
    assert_eq!(Foo::Variant, back);
}
