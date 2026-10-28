use proc_macro::TokenStream;
use quote::quote;
use syn::{parse_macro_input, ItemFn, FnArg, PatType};

fn create_wrapper(item: TokenStream, prefix: &str, is_reg: bool, bundle_type: &str) -> TokenStream {
    let input = parse_macro_input!(item as ItemFn);
    let orig_name = &input.sig.ident;
    let wrapper_name = syn::Ident::new(&format!("{}{}", prefix, orig_name), orig_name.span());
    let generics = &input.sig.generics;
    let arg_type = match input.sig.inputs.first().expect("Function requires at least one argument") {
        FnArg::Typed(PatType { ty, .. }) => ty,
        _ => panic!("Expected a typed argument"),
    };
    let orig_fn = &input;
    let err_handler = match bundle_type {
        "msg" => quote! {
            let err_msg = format!("❌ ERROR on command [{}]: {:?}", stringify!(#orig_name), why);
            println!("{}", err_msg);
            let _ = bnd.msg.reply(&bnd.ctx.http, &err_msg).await;
        },
        "auto" | "combine" => quote! {
            // Expired-interaction noise (10062/40060) is expected after
            // restarts or user cancels; log it briefly instead of an ERROR.
            if crate::reusable::component::new_error::is_benign_interaction_error(&why) {
                println!("⚠️ Expired interaction ignored on command [{}]", stringify!(#orig_name));
                return;
            }
            let err_msg = format!("❌ ERROR on command [{}]: {:?}", stringify!(#orig_name), why);
            println!("{}", err_msg);
        },
        _ => quote! {
            // Expired-interaction noise (10062/40060) is expected after
            // restarts or user cancels; the interaction is already dead, so
            // skip the user-facing error embed entirely.
            if crate::reusable::component::new_error::is_benign_interaction_error(&why) {
                println!("⚠️ Expired interaction ignored on command [{}]", stringify!(#orig_name));
                return;
            }
            let err_msg_str = format!("{:?}", why);
            println!("❌ ERROR on command [{}]: {}", stringify!(#orig_name), err_msg_str);
            let err_user = crate::event::interaction::Mybundle::user(bnd);
            let mut err_log = crate::reusable::component::error::ErrorLog::new(bnd.ctx, bnd.init, &err_user).await;
            err_log.change_error(err_msg_str, stringify!(#orig_name).to_string(), String::new());
            let res = bnd.cmd.create_response(&bnd.ctx.http, err_log.interaction_response(false, true)).await;
            if res.is_err() {
                let _ = bnd.cmd.edit_response(&bnd.ctx.http, err_log.defer_response(true)).await;
            }
        },
    };
    if is_reg {
        TokenStream::from(quote! {
            #orig_fn
            pub async fn #wrapper_name #generics (bnd: #arg_type) {
                let user = crate::event::interaction::Mybundle::user(bnd);
                if let Ok(reg) = crate::reusable::component::registered::Reg::check(bnd, &user).await {
                    if let Err(why) = #orig_name(bnd, &reg).await {
                        #err_handler
                    }
                } else {
                    println!("⚠️ User is not registered: {}", user.name);
                }
            }
        })
    } else {
        TokenStream::from(quote! {
            #orig_fn
            pub async fn #wrapper_name #generics (bnd: #arg_type) {
                if let Err(why) = #orig_name(bnd).await {
                    #err_handler
                }
            }
        })
    }
}

#[proc_macro_attribute]
pub fn hertz_slash_normal(_: TokenStream, item: TokenStream) -> TokenStream {
    create_wrapper(item, "discord", false, "slash")
}

#[proc_macro_attribute]
pub fn hertz_slash_reg(_: TokenStream, item: TokenStream) -> TokenStream {
    create_wrapper(item, "discord", true, "slash")
}

#[proc_macro_attribute]
pub fn hertz_button_normal(_: TokenStream, item: TokenStream) -> TokenStream {
    create_wrapper(item, "discord", false, "button")
}

#[proc_macro_attribute]
pub fn hertz_combine_normal(_: TokenStream, item: TokenStream) -> TokenStream {
    create_wrapper(item, "discord", false, "combine")
}

#[proc_macro_attribute]
pub fn hertz_modal_normal(_: TokenStream, item: TokenStream) -> TokenStream {
    create_wrapper(item, "discord", false, "modal")
}

#[proc_macro_attribute]
pub fn hertz_auto(_: TokenStream, item: TokenStream) -> TokenStream {
    create_wrapper(item, "discord", false, "auto")
}

#[proc_macro_attribute]
pub fn hertz_msg(_: TokenStream, item: TokenStream) -> TokenStream {
    create_wrapper(item, "discord", false, "msg")
}

#[proc_macro_attribute]
pub fn hertz_combine_reg(_: TokenStream, item: TokenStream) -> TokenStream {
    create_wrapper(item, "discord", true, "combine")
}