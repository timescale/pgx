use proc_macro2::TokenStream as TokenStream2;
use quote::{quote, ToTokens, TokenStreamExt};
use syn::spanned::Spanned;
use syn::{AttrStyle, Attribute, Lit, Meta, MetaList, NestedMeta};

#[derive(Debug, Clone)]
pub struct ToSqlConfig {
    enabled: bool,
    callback: Option<syn::Path>,
}
impl Default for ToSqlConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            callback: None,
        }
    }
}

const INVALID_ATTR_CONTENT: &str =
    "expected either #[to_sql(bool)] or #[to_sql(path::to::callback)]";

impl ToSqlConfig {
    /// Used for general purpose parsing from an attribute
    pub fn from_attribute(attr: &Attribute) -> Result<Self, syn::Error> {
        if attr.style != AttrStyle::Outer {
            return Err(syn::Error::new(
                attr.span(),
                "#[to_sql(..)] is only valid in an outer context",
            ));
        }

        let mut enabled = true;
        let mut callback: Option<syn::Path> = None;

        match attr.parse_meta()? {
            Meta::List(MetaList { nested, .. }) => {
                let meta = nested.first().ok_or_else(|| {
                    syn::Error::new(nested.span(), "expected non-empty argument list")
                })?;
                if nested.len() > 1 {
                    return Err(syn::Error::new(nested.span(), INVALID_ATTR_CONTENT));
                }
                match meta {
                    NestedMeta::Lit(Lit::Bool(b)) => {
                        enabled = b.value;
                    }
                    NestedMeta::Lit(lit) => {
                        return Err(syn::Error::new(lit.span(), INVALID_ATTR_CONTENT))
                    }
                    NestedMeta::Meta(Meta::Path(callback_path)) => {
                        callback = Some(callback_path.clone());
                    }
                    NestedMeta::Meta(meta) => {
                        return Err(syn::Error::new(meta.span(), INVALID_ATTR_CONTENT))
                    }
                }
            }
            _ => return Err(syn::Error::new(attr.span(), "expected argument list")),
        }

        Ok(Self { enabled, callback })
    }

    /// Used to parse a generator config from a set of item attributes
    pub fn from_attributes(attrs: &[Attribute]) -> Result<Option<Self>, syn::Error> {
        if let Some(attr) = attrs.iter().find(|attr| attr.path.is_ident("to_sql")) {
            Self::from_attribute(attr).map(Some)
        } else {
            Ok(None)
        }
    }
}

impl ToTokens for ToSqlConfig {
    fn to_tokens(&self, tokens: &mut TokenStream2) {
        let enabled = self.enabled;
        let callback = &self.callback;
        let quoted = if let Some(callback_path) = callback {
            quote! {
                ::pgx::datum::sql_entity_graph::ToSqlConfig {
                    enabled: #enabled,
                    callback: Some(#callback_path),
                }
            }
        } else {
            quote! {
                ::pgx::datum::sql_entity_graph::ToSqlConfig {
                    enabled: #enabled,
                    callback: None,
                }
            }
        };
        tokens.append_all(quoted);
    }
}
