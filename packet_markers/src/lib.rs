use proc_macro::TokenStream;
use quote::quote;
use syn::{parse_macro_input, DeriveInput};

/// Derive macro for the PacketReadable marker trait
#[proc_macro_derive(PacketReadable)]
pub fn derive_packet_readable(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    let name = &input.ident;

    let expanded = quote! {
        impl PacketReadable for #name {}
    };

    TokenStream::from(expanded)
}

/// Derive macro for the PacketWritable marker trait
#[proc_macro_derive(PacketWritable)]
pub fn derive_packet_writable(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    let name = &input.ident;

    let expanded = quote! {
        impl PacketWritable for #name {}
    };

    TokenStream::from(expanded)
}