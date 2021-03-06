use proc_macro::TokenStream;
mod import;
mod modules;
#[proc_macro]
pub fn autoimport(input: TokenStream) -> TokenStream {
    let tokens = import::autoimport(proc_macro2::TokenStream::from(input));
    TokenStream::from(tokens)
}
