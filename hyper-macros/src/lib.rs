use proc_macro::TokenStream;
use quote::quote;
use syn::{
    parse_macro_input, parse_quote, Arm, AttrStyle, Attribute, Data, DeriveInput, Expr, Field,
    Fields, GenericArgument, Lit, Meta, PathArguments, Type, TypePath,
};

#[proc_macro_derive(Hyperparams, attributes(hyper))]
pub fn hyperparams_derive(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    let name = &input.ident;

    let (impl_generics, ty_generics, where_clause) = input.generics.split_for_impl();

    let fields = match &input.data {
        Data::Struct(s) => match &s.fields {
            Fields::Named(f) => &f.named,
            _ => unimplemented!("Only named fields are supported"),
        },
        _ => unimplemented!("Only structs are supported"),
    };

    let meta_fields = fields.iter().filter_map(|f| {
        if should_skip(&f.attrs) {
            return None;
        }
        let field_name = f.ident.as_ref().unwrap();
        let field_name_str = field_name.to_string();
        let ty = &f.ty;

        let (schema_gen, default_gen) = type_to_schema_and_default(ty, &f.attrs);

        Some(quote! {
            {
                let field_name = #field_name_str.to_string();
                let schema = #schema_gen;
                let default_value = #default_gen(&defaults.#field_name);
                let meta = ParamMeta {
                    default: default_value,
                    schema: schema,
                    description: None, // TODO: Add doc comment support
                };
                map.insert(field_name, meta);
            }
        })
    });

    let expanded = quote! {
        impl #impl_generics Hyperparams for #name #ty_generics #where_clause {
            fn metadata() -> std::collections::HashMap<String, ParamMeta> {
                use crate::hyper::{ParamMeta, ParamSchema, ParamValue};
                let mut map = std::collections::HashMap::new();
                let defaults = Self::default();

                #(#meta_fields)*

                map
            }
        }
    };

    TokenStream::from(expanded)
}

fn should_skip(attrs: &[Attribute]) -> bool {
    attrs.iter().any(|attr| {
        if attr.path().is_ident("hyper") || attr.path().is_ident("serde") {
            if let Ok(syn::Meta::List(meta_list)) = attr.meta.clone() {
                return meta_list.tokens.to_string() == "skip";
            }
        }
        false
    })
}

fn type_to_schema_and_default(
    ty: &Type,
    attrs: &[Attribute],
) -> (proc_macro2::TokenStream, proc_macro2::TokenStream) {
    let mut range_expr: Option<Expr> = None;
    let mut element_range_expr: Option<Expr> = None;

    for attr in attrs {
        if attr.path().is_ident("hyper") {
            if let Ok(meta) = attr.meta.require_list() {
                if let Ok(expr) = meta.parse_args::<Expr>() {
                    // Simple `#[hyper(range(min..max))]` attribute
                    range_expr = Some(expr);
                }
            }
        }
    }

    match ty {
        Type::Path(type_path) => {
            if let Some(ident) = type_path.path.get_ident() {
                let schema = match ident.to_string().as_str() {
                    "u8" | "u16" | "u32" | "u64" | "usize" => {
                        let (min, max) = parse_range_attr(range_expr, "0", "u64::MAX");
                        quote! { ParamSchema::Uint { min: #min, max: #max } }
                    }
                    "i8" | "i16" | "i32" | "i64" | "isize" => {
                        let (min, max) = parse_range_attr(range_expr, "i64::MIN", "i64::MAX");
                        quote! { ParamSchema::Int { min: #min, max: #max } }
                    }
                    "f32" | "f64" => {
                        let (min, max) = parse_range_attr(range_expr, "f64::MIN", "f64::MAX");
                        quote! { ParamSchema::Float { min: #min, max: #max } }
                    }
                    "bool" => quote! { ParamSchema::Bool },
                    "String" => quote! {
                        ParamSchema::Enum { options: vec![] } // Placeholder
                    },
                    _ => generate_complex_schema(type_path, range_expr, element_range_expr),
                };

                let default_converter = match ident.to_string().as_str() {
                    "u8" | "u16" | "u32" | "u64" | "usize" => {
                        quote! { |val| ParamValue::Uint(*val as u64) }
                    }
                    "i8" | "i16" | "i32" | "i64" | "isize" => {
                        quote! { |val| ParamValue::Int(*val as i64) }
                    }
                    "f32" | "f64" => quote! { |val| ParamValue::Float(*val as f64) },
                    "bool" => quote! { |val| ParamValue::Bool(*val) },
                    "String" => quote! { |val: &String| ParamValue::String(val.clone()) },
                    _ => generate_complex_default(type_path),
                };

                return (schema, default_converter);
            }

            // Handle types like `Vec<T>` and `HashMap<K, V>`
            let last_segment = type_path.path.segments.last().unwrap();
            let type_name = &last_segment.ident;

            if type_name == "Vec" {
                if let PathArguments::AngleBracketed(args) = &last_segment.arguments {
                    if let Some(syn::GenericArgument::Type(element_ty)) = args.args.first() {
                        let (element_schema, element_default) =
                            type_to_schema_and_default(element_ty, &[]);
                        let schema = quote! {
                            ParamSchema::List {
                                element_schema: Box::new(ParamMeta {
                                    schema: #element_schema,
                                    default: (#element_default)(&<#element_ty>::default()),
                                    description: None,
                                })
                            }
                        };
                        let default_converter = quote! {
                            |val: &Vec<#element_ty>| ParamValue::List(
                                val.iter().map(#element_default).collect()
                            )
                        };
                        return (schema, default_converter);
                    }
                }
            }

            if type_name == "HashMap" {
                if let PathArguments::AngleBracketed(args) = &last_segment.arguments {
                    if let (Some(syn::GenericArgument::Type(key_ty)), Some(syn::GenericArgument::Type(value_ty))) =
                        (args.args.first(), args.args.last())
                    {
                        let (key_schema, key_default) = type_to_schema_and_default(key_ty, &[]);
                        let (value_schema, value_default) =
                            type_to_schema_and_default(value_ty, &[]);

                        let schema = quote! {
                            ParamSchema::Map {
                                key_schema: Box::new(ParamMeta {
                                    schema: #key_schema,
                                    default: (#key_default)(&<#key_ty>::default()),
                                    description: None,
                                }),
                                value_schema: Box::new(ParamMeta {
                                    schema: #value_schema,
                                    default: (#value_default)(&<#value_ty>::default()),
                                    description: None,
                                })
                            }
                        };
                        let default_converter = quote! {
                            |val: &std::collections::HashMap<#key_ty, #value_ty>| {
                                 ParamValue::Map(
                                     val.iter().map(|(k,v)| (
                                         match (#key_default)(k) {
                                             ParamValue::String(s) => s,
                                             ParamValue::Uint(u) => u.to_string(),
                                             ParamValue::Int(i) => i.to_string(),
                                             _ => panic!("Unsupported map key type")
                                         },
                                         (#value_default)(v)
                                     )).collect()
                                )
                            }
                        };
                        return (schema, default_converter);
                    }
                }
            }

            (
                generate_complex_schema(type_path, range_expr, element_range_expr),
                generate_complex_default(type_path),
            )
        }
        Type::Array(type_array) => {
            let element_ty = &type_array.elem;
            let (element_schema, element_default) = type_to_schema_and_default(element_ty, &[]);
            let schema = quote! {
                ParamSchema::List {
                    element_schema: Box::new(ParamMeta {
                        schema: #element_schema,
                        default: (#element_default)(&<#element_ty>::default()),
                        description: None,
                    })
                }
            };
            let default_converter = quote! {
                |val: &[#element_ty]| ParamValue::List(
                    val.iter().map(#element_default).collect()
                )
            };
            (schema, default_converter)
        }
        _ => (
            quote! { ParamSchema::Unsupported(stringify!(#ty).to_string()) },
            quote! { |_| ParamValue::String("unsupported".to_string()) },
        ),
    }
}

fn parse_range_attr(
    range_expr: Option<Expr>,
    default_min: &str,
    default_max: &str,
) -> (proc_macro2::TokenStream, proc_macro2::TokenStream) {
    let min_expr = proc_macro2::TokenStream::from_str(default_min).unwrap();
    let max_expr = proc_macro2::TokenStream::from_str(default_max).unwrap();

    if let Some(Expr::Range(expr_range)) = range_expr {
        let from = expr_range
            .start
            .as_ref()
            .map_or(min_expr.clone(), |e| quote! { #e });
        let to = expr_range
            .end
            .as_ref()
            .map_or(max_expr.clone(), |e| quote! { #e });
        (from, to)
    } else {
        (min_expr, max_expr)
    }
}

fn generate_complex_schema(
    type_path: &TypePath,
    _range: Option<Expr>,
    _element_range: Option<Expr>,
) -> proc_macro2::TokenStream {
    let ty = Type::Path(type_path.clone());
    // This is a heuristic. A better way would be a helper attribute.
    let is_enum = {
        let last_segment = type_path.path.segments.last().unwrap();
        let name = last_segment.ident.to_string();
        name.chars().next().map_or(false, |c| c.is_uppercase())
            && name != "Vec"
            && name != "HashMap"
    };

    if is_enum {
        quote! {
            {
                use strum::IntoEnumIterator;
                ParamSchema::Enum {
                    options: #ty::iter().map(|v| serde_json::to_string(&v).unwrap().trim_matches('"').to_string()).collect()
                }
            }
        }
    } else {
        // Assumes it's a struct that derives Hyperparams
        quote! {
            ParamSchema::Struct {
                fields: <#ty>::metadata()
            }
        }
    }
}

fn generate_complex_default(type_path: &TypePath) -> proc_macro2::TokenStream {
    let ty = Type::Path(type_path.clone());
    let is_enum = {
        let last_segment = type_path.path.segments.last().unwrap();
        let name = last_segment.ident.to_string();
        name.chars().next().map_or(false, |c| c.is_uppercase())
            && name != "Vec"
            && name != "HashMap"
    };

    if is_enum {
        quote! {
            |val: &#ty| ParamValue::String(serde_json::to_string(val).unwrap().trim_matches('"').to_string())
        }
    } else {
        // It's a struct
        quote! {
             |val: &#ty| {
                let meta = <#ty as Hyperparams>::metadata(&val);
                ParamValue::Struct(
                    meta.into_iter().map(|(k, v)| (k, v.default)).collect()
                )
            }
        }
    }
}
