use proc_macro::TokenStream;

use quote::{format_ident, quote};
use syn::parse::{Parse, ParseStream};
use syn::{parse_macro_input, Ident, ItemFn, LitStr, Token};

struct CommandArgs {
    name: LitStr,
    admin: bool,
}

impl Parse for CommandArgs {
    fn parse(input: ParseStream<'_>) -> syn::Result<Self> {
        let name = input.parse::<LitStr>()?;
        let mut admin = false;
        if input.peek(Token![,]) {
            input.parse::<Token![,]>()?;
            let flag = input.parse::<Ident>()?;
            match flag.to_string().as_str() {
                "admin" => admin = true,
                other => {
                    return Err(syn::Error::new(
                        flag.span(),
                        format!("不认识参数 {other:?}, 只支持 admin"),
                    ))
                }
            }
        }
        if !input.is_empty() {
            return Err(input.error("参数过多, 只支持命令名与 admin"));
        }
        Ok(Self { name, admin })
    }
}

#[proc_macro_attribute]
pub fn command(attr: TokenStream, item: TokenStream) -> TokenStream {
    let args = parse_macro_input!(attr as CommandArgs);
    let function = parse_macro_input!(item as ItemFn);
    let const_name = format_ident!(
        "__AURORIX_CMD_{}",
        function.sig.ident.to_string().to_uppercase()
    );
    let command_name = args.name.value();
    let admin = args.admin;
    let expanded = quote! {
        #function
        pub const #const_name: (&'static str, bool) = (#command_name, #admin);
    };
    expanded.into()
}
