extern crate proc_macro;

use darling::ast::NestedMeta;
use darling::FromMeta;
use proc_macro2::TokenStream;


use syn::{parse_macro_input, AttrStyle, Expr, ExprLit, Fields, ItemEnum, Lit, Meta, Variant};
use quote::{format_ident, quote};
use syn::spanned::Spanned;


/// Takes a C-Style Rust enum and creates an FFI safe representation and safe conversions
///
/// Given a C-Style Rust enum (no fields), an FFI safe newtype representation is created
/// with the same name as the original enum and conversion methods to and from the safe rust
/// enum are created. The safe rust enum gets an additional catch-all variant.
///
/// This macro is intended to be used together with bindgen rustified enums, so that
/// the FFI-safe type is used in the FFI, but conversions to the safe rust enum are simple.
#[proc_macro_attribute]
pub fn enum_ffi(args: proc_macro::TokenStream, input: proc_macro::TokenStream) -> proc_macro::TokenStream {
    match enum_ffi_newtype(parse_macro_input!(input), args.into()) {
        Ok(output) => output.into(),
        Err(err) => err.to_compile_error().into(),
    }
}

fn evaluate_discriminant_expr(discriminant: &Expr) -> Result<i64, syn::Error> {
    if let Expr::Lit(ExprLit {
        lit: Lit::Int(lit_int),
        ..
                     }) = discriminant {
        lit_int.base10_parse()
    } else {
        Err(syn::Error::new(discriminant.span(), "discriminant must be an integer"))
    }
}

fn get_enum_repr(item_enum: &ItemEnum) -> Result<TokenStream, syn::Error> {
    item_enum.attrs.iter()
        .filter(|attr| matches!(attr.style, AttrStyle::Outer))
        .find_map(|attr| {
            if let Meta::List(list) = &attr.meta {
                let ident = list.path.get_ident().map(|ident| ident.to_string())?;
                if ident != "repr" {
                    return None;
                }
                Some(list.tokens.clone())
            } else {
                None
            }
        })
        .ok_or(syn::Error::new(item_enum.span(), "No `repr` attribute found."))
}

#[derive(Debug, FromMeta)]
struct MacroArgs {
    /// Let the FFI enum be represented by a NonZero type
    ///
    /// This is mainly useful for C-Result enums, where 0 is the success case.
    /// In this case we want our Rust enum to not contain the success variant,
    /// and instead have a NonZero error enum. The type used in the FFI can
    /// then be `Result<(), NonZeroFfiEnum>`.
    #[darling(default)]
    non_zero: bool,
    /// A fallback catch-all enum variant of the rustified enum
    ///
    /// If not specified, this macro will inject a new catch-all variant at
    /// the end of the enum.
    /// If the specified enum variant already exists, then no new variant will be created,
    /// but it is still used as the catch-all fallback.
    catch_all: Option<String>,
    /// The identifier the safe Rust enum should have
    ///
    /// The newtype FFI enum will get the original enum name.
    rust_enum_name: Option<String>,
}

fn enum_ffi_newtype(item_enum: ItemEnum, macro_args: TokenStream) -> Result<TokenStream, syn::Error> {
    let original_ident = item_enum.ident.clone();
    let attr_args = NestedMeta::parse_meta_list(macro_args)?;
    let macro_args = MacroArgs::from_list(&attr_args)?;
    let mut curr_discriminant = 0;
    let mut newtype_variants = vec![];
    let mut newtype_variant_idents = vec![];


    // The representation.
    let base_repr_tokens = get_enum_repr(&item_enum)?;

    let repr_tokens = if macro_args.non_zero {
        quote! { core::num::NonZero<#base_repr_tokens> }
    } else {
        base_repr_tokens.clone()
    };

    for variant in &item_enum.variants {
        if let Some((_, discriminant)) = &variant.discriminant {
            curr_discriminant = evaluate_discriminant_expr(&discriminant)?;
        }
        if macro_args.non_zero && curr_discriminant == 0 {
            return Err(syn::Error::new(variant.span(), "discriminant must not be zero for NonZero representation"));
        }
        let variant_ident = &variant.ident;
        newtype_variant_idents.push(variant_ident.clone());
        if !variant.fields.is_empty() {
            return Err(syn::Error::new(variant.fields.span(), "FFI Enum variants may not contain fields"));
        }

        let lit_value = proc_macro2::Literal::i64_unsuffixed(curr_discriminant);
        let value = if macro_args.non_zero {
            quote! { const { #original_ident(core::num::NonZero::new(#lit_value).unwrap()) } }
        } else {
            quote! { #original_ident(#lit_value) }
        };
        newtype_variants.push(
            quote!{
                    pub const #variant_ident: #original_ident = #value;
                }
        );
        curr_discriminant += 1;
    }

    let vis = &item_enum.vis;

    let mut rust_enum = item_enum.clone();
    let rust_enum_ident = macro_args.rust_enum_name
        .map(|name| format_ident!("{}", name) )
        .unwrap_or(format_ident!("{}Rustified", original_ident));
    rust_enum.ident = rust_enum_ident.clone();
    let catch_all_ident = if let Some(catch_all_variant) = &macro_args.catch_all {
        let variant_exists = rust_enum.variants.iter().find(|variant| &variant.ident.to_string() == catch_all_variant).is_some();
        let catch_all_ident = format_ident!("{}", catch_all_variant);
        if !variant_exists {
            rust_enum.variants.push(Variant {
                attrs: vec![],
                ident: catch_all_ident.clone(),
                fields: Fields::Unit,
                discriminant: None,
            });
        }
        catch_all_ident
    } else {
        let catch_all_ident = format_ident!("UnknownVariant{}", original_ident);
        rust_enum.variants.push(Variant {
            attrs: vec![],
            ident: catch_all_ident.clone(),
            fields: Fields::Unit,
            discriminant: None,
        });
        catch_all_ident
    };

    let rust_enum_to_ffi_conversion = if macro_args.non_zero {
        quote! {
            // SAFETY: We know that all #rust_enum_ident values are NonZero.
            unsafe { core::num::NonZero::new_unchecked(value as #base_repr_tokens) }
        }
    } else {
        quote!{ value as #repr_tokens }
    };



    Ok(quote! {
        #rust_enum

        const _: () = const { assert!(#rust_enum_ident::#catch_all_ident as u64 != 0 ); };

        // todo: take derives from parent enum.
        #[repr(transparent)]
        #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
        #vis struct #original_ident(pub #repr_tokens);

        #[allow(non_upper_case_globals)]
        impl #original_ident {
            #(#newtype_variants)*
        }

        impl From<#rust_enum_ident> for #original_ident {
            fn from(value: #rust_enum_ident) -> Self {
                Self(#rust_enum_to_ffi_conversion)
            }
        }

        impl From<#original_ident> for #rust_enum_ident {
            fn from(value: #original_ident) -> Self {
                match value {
                    #( x if x == #original_ident::#newtype_variant_idents => #rust_enum_ident::#newtype_variant_idents),*,
                    _ => #rust_enum_ident::#catch_all_ident,
                }
            }
        }
    })
}