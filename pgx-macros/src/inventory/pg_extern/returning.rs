use proc_macro2::TokenStream as TokenStream2;
use quote::{quote, ToTokens, TokenStreamExt};
use std::{convert::TryFrom, ops::Deref};
use syn::{
    parse::{Parse, ParseStream},
    Token,
};

#[derive(Debug, Clone)]
pub enum Returning {
    None,
    Type(syn::Type),
    SetOf(syn::TypePath),
    Iterated(Vec<(syn::Type, Option<proc_macro2::Ident>)>),
}

impl TryFrom<&syn::ReturnType> for Returning {
    type Error = ();

    fn try_from(value: &syn::ReturnType) -> Result<Self, Self::Error> {
        Ok(match &value {
            syn::ReturnType::Default => Returning::None,
            syn::ReturnType::Type(_, ty) => match *ty.clone() {
                syn::Type::ImplTrait(impl_trait) => match impl_trait.bounds.first().unwrap() {
                    syn::TypeParamBound::Trait(trait_bound) => {
                        let last_path_segment = trait_bound.path.segments.last().unwrap();
                        match last_path_segment.ident.to_string().as_str() {
                            "Iterator" => match &last_path_segment.arguments {
                                syn::PathArguments::AngleBracketed(args) => {
                                    match args.args.first().unwrap() {
                                        syn::GenericArgument::Binding(binding) => match &binding.ty
                                        {
                                            syn::Type::Tuple(tuple_type) => {
                                                let returns: Vec<(syn::Type, Option<syn::Ident>)> = tuple_type.elems.iter().flat_map(|elem| {
                                                    match elem {
                                                        syn::Type::Macro(macro_pat) => {
                                                            let mac = &macro_pat.mac;
                                                            let archetype = mac.path.segments.last().unwrap();
                                                            match archetype.ident.to_string().as_str() {
                                                                "name" => {
                                                                    let out: NameMacro = mac.parse_body().expect(&*format!("{:?}", mac));
                                                                    Some((out.ty, Some(out.ident)))
                                                                },
                                                                _ => unimplemented!("Don't support anything other than name."),
                                                            }
                                                        },
                                                        ty => Some((ty.clone(), None)),
                                                    }
                                                }).collect();
                                                Returning::Iterated(returns)
                                            },
                                            syn::Type::Path(path) => {
                                                Returning::SetOf(path.clone())
                                            },
                                            ty => unimplemented!("Only iters with tuples, got {:?}.", ty),
                                        },
                                        _ => unimplemented!(),
                                    }
                                }
                                _ => unimplemented!(),
                            },
                            _ => unimplemented!(),
                        }
                    }
                    _ => Returning::None,
                },
                _ => Returning::Type(ty.deref().clone()),
            },
        })
    }
}

impl ToTokens for Returning {
    fn to_tokens(&self, tokens: &mut TokenStream2) {
        let quoted = match self {
            Returning::None => quote! {
                crate::__pgx_internals::PgxExternReturn::None
            },
            Returning::Type(ty) => quote! {
                crate::__pgx_internals::PgxExternReturn::Type {
                    id: TypeId::of::<#ty>(),
                    name: core::any::type_name::<#ty>(),
                }
            },
            Returning::SetOf(ty) => quote! {
                crate::__pgx_internals::PgxExternReturn::SetOf {
                    id: TypeId::of::<#ty>(),
                    name: core::any::type_name::<#ty>(),
                }
            },
            Returning::Iterated(items) => {
                let quoted_items = items
                    .iter()
                    .map(|(ty, name)| {
                        let name_iter = name.iter();
                        quote! {
                            (
                                TypeId::of::<#ty>(),
                                core::any::type_name::<#ty>(),
                                None#( .unwrap_or(Some(stringify!(#name_iter))) )*,
                            )
                        }
                    })
                    .collect::<Vec<_>>();
                quote! {
                    crate::__pgx_internals::PgxExternReturn::Iterated(vec![
                        #(#quoted_items),*
                    ])
                }
            }
        };
        tokens.append_all(quoted);
    }
}

#[derive(Debug)]
pub(crate) struct NameMacro {
    pub(crate) ident: syn::Ident,
    comma: Token![,],
    pub(crate) ty: syn::Type,
}

impl Parse for NameMacro {
    fn parse(input: ParseStream) -> Result<Self, syn::Error> {
        Ok(Self {
            ident: input.parse()?,
            comma: input.parse()?,
            ty: input.parse()?,
        })
    }
}
