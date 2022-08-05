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
                    fields.push(quote!(stringify!(#ident)));
                    match_arms.push(quote!(stringify!(#ident) => Self {
                        #ident: param.parse().unwrap_or_default(),
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
