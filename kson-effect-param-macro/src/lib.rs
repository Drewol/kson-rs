use proc_macro::TokenStream;
use quote::quote;
use syn::{parse_macro_input, Data, DeriveInput};

#[proc_macro_derive(DeriveParameter)]
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
                        quote!(#input::#variant(DeriveParameter::derive(&a,&b)))
                    }
                    syn::Fields::Unit => panic!("Enum unit variants are not supported"),
                };
                let input = &input.ident;
                let variant = &variant.ident;
                match_arms
                    .push(quote!((#input::#variant(a), #input::#variant(b)) => #new_struct_expr,));
            }

            match_arms.push(quote!(_ => panic!("Tried to derive from a different type"),));
            let input = &input.ident;
            proc_macro::TokenStream::from(quote!(
                impl DeriveParameter for #input {
                fn derive(&self, other: &Self) -> Self {
                    match (self,other) { #(#match_arms)* }
                }
                }
            ))
        }
        Data::Struct(s) => {
            //self << other
            let mut field_initializers = vec![];

            for f in &s.fields {
                if let Some(ident) = &f.ident {
                    field_initializers.push(quote!(#ident: self.#ident.derive(&other.#ident),))
                }
            }
            let input = &input.ident;
            proc_macro::TokenStream::from(quote!(
            impl DeriveParameter for #input {
                fn derive(&self, other: &Self) -> Self {
                    Self {#(#field_initializers)*}
                }
            }))
        }
        Data::Union(_) => panic!("Unions are not supported"),
    }
}
