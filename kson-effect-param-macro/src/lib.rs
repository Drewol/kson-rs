use proc_macro::TokenStream;
use quote::quote;
use syn::{parse_macro_input, Data, DeriveInput};

#[proc_macro_derive(Effect)]
pub fn derive_effect_param(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);

    match &input.data {
        Data::Enum(e) => {
            let mut match_arms = vec![];
            for variant in &e.variants {
                let new_struct_expr = match &variant.fields {
                    syn::Fields::Named(_) => todo!(),
                    syn::Fields::Unnamed(_) => {
                        let input = &input.ident;
                        let variant = &variant.ident;
                        quote!(#input::#variant(Effect::derive(&a,key,param)))
                    }
                    syn::Fields::Unit => panic!("Enum unit variants are not supported"),
                };
                let input = &input.ident;
                let variant = &variant.ident;
                match_arms.push(quote!(#input::#variant(a) => #new_struct_expr,));
            }

            match_arms.push(quote!(_ => panic!("Tried to derive from a different type"),));
            let input = &input.ident;
            proc_macro::TokenStream::from(quote!(
                impl Effect for #input {
                fn derive(&self, key: &str, param: &str) -> Self {
                    match self { #(#match_arms)* }
                }
                fn param_list() -> &'static [&'static str] {
                    &[]
                }
                }
            ))
        }
        Data::Struct(s) => {
            //self << other
            let mut match_arms = vec![];
            let mut fields = vec![];

            for f in &s.fields {
                if let Some(ident) = &f.ident {
                    let name = ident.to_string();
                    let aliases = serde_aliases(&f.attrs);
                    fields.push(quote!(#name));
                    // Each field is parsed as the spec type of its parameter name.
                    // Invalid values are ignored and the previous value is kept.
                    match_arms.push(quote!(#name #(| #aliases)* => Self {
                        #ident: crate::parameter::ParamField::parse_field(
                            param,
                            crate::parameter::ParamType::for_name(#name),
                        )
                        .unwrap_or_else(|| self.#ident.clone()),
                        ..self.clone()
                    },))
                }
            }

            match_arms.push(quote!(_ => self.clone()));

            let input = &input.ident;
            proc_macro::TokenStream::from(quote!(
            impl Effect for #input {
                fn derive(&self, key: &str, param: &str) -> Self {
                    match key {#(#match_arms)*}
                }

                fn param_list() -> &'static [&'static str]  {
                    &[#(#fields),*]
                }
            }))
        }
        Data::Union(_) => panic!("Unions are not supported"),
    }
}

/// Collects the values of `#[serde(alias = "...")]` attributes
fn serde_aliases(attrs: &[syn::Attribute]) -> Vec<String> {
    let mut aliases = vec![];
    for attr in attrs.iter().filter(|a| a.path().is_ident("serde")) {
        _ = attr.parse_nested_meta(|meta| {
            if meta.path.is_ident("alias") {
                let lit: syn::LitStr = meta.value()?.parse()?;
                aliases.push(lit.value());
            } else if meta.input.peek(syn::Token![=]) {
                // Skip the value of other `key = value` items
                let _: syn::Expr = meta.value()?.parse()?;
            }
            Ok(())
        });
    }
    aliases
}
