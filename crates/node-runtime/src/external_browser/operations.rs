//! Managed browser primitives; tool declarations and transformations belong to packages.
use adk_browser::BrowserSession;
use sailry_protocol::external_browser::Action;
use serde_json::Value;

mod actions;
mod click;
mod cookies;
mod evaluate;
mod extract;
mod frames;
mod navigate;
mod screenshot;
mod type_text;
mod wait;
mod windows;

pub(super) async fn execute(
    browser: &BrowserSession,
    action: Action,
    arguments: Value,
) -> adk_core::Result<Value> {
    match action {
        Action::DragAndDrop => actions::drag_and_drop(browser, arguments).await,
        Action::RightClick => actions::right_click(browser, arguments).await,
        Action::Focus => actions::focus(browser, arguments).await,
        Action::ElementState => actions::element_state(browser, arguments).await,
        Action::PressKey => actions::press_key(browser, arguments).await,
        Action::FileUpload => actions::file_upload(browser, arguments).await,
        Action::PrintToPdf => actions::print_to_pdf(browser, arguments).await,
        Action::Click => click::click(browser, arguments).await,
        Action::DoubleClick => click::double_click(browser, arguments).await,
        Action::GetCookies => cookies::get_cookies(browser, arguments).await,
        Action::GetCookie => cookies::get_cookie(browser, arguments).await,
        Action::AddCookie => cookies::add_cookie(browser, arguments).await,
        Action::DeleteCookie => cookies::delete_cookie(browser, arguments).await,
        Action::DeleteAllCookies => cookies::delete_all_cookies(browser, arguments).await,
        Action::EvaluateJs => evaluate::script(browser, arguments).await,
        Action::Scroll => evaluate::scroll(browser, arguments).await,
        Action::Hover => evaluate::hover(browser, arguments).await,
        Action::HandleAlert => evaluate::handle_alert(browser, arguments).await,
        Action::ExtractText => extract::text(browser, arguments).await,
        Action::ExtractAttribute => extract::attribute(browser, arguments).await,
        Action::ExtractLinks => extract::links(browser, arguments).await,
        Action::PageInfo => extract::page_info(browser, arguments).await,
        Action::PageSource => extract::page_source(browser, arguments).await,
        Action::SwitchToFrame => frames::switch_to_frame(browser, arguments).await,
        Action::SwitchToParentFrame => frames::switch_to_parent_frame(browser, arguments).await,
        Action::SwitchToDefaultContent => {
            frames::switch_to_default_content(browser, arguments).await
        }
        Action::Navigate => navigate::navigate(browser, arguments).await,
        Action::Back => navigate::back(browser, arguments).await,
        Action::Forward => navigate::forward(browser, arguments).await,
        Action::Refresh => navigate::refresh(browser, arguments).await,
        Action::Screenshot => screenshot::screenshot(browser, arguments).await,
        Action::Type => type_text::r#type(browser, arguments).await,
        Action::Clear => type_text::clear(browser, arguments).await,
        Action::Select => type_text::select(browser, arguments).await,
        Action::WaitForElement => wait::element(browser, arguments).await,
        Action::Wait => wait::wait(browser, arguments).await,
        Action::WaitForPageLoad => wait::page_load(browser, arguments).await,
        Action::WaitForText => wait::text(browser, arguments).await,
        Action::ListWindows => windows::list_windows(browser, arguments).await,
        Action::NewTab => windows::new_tab(browser, arguments).await,
        Action::NewWindow => windows::new_window(browser, arguments).await,
        Action::SwitchWindow => windows::switch_window(browser, arguments).await,
        Action::CloseWindow => windows::close_window(browser, arguments).await,
        Action::MaximizeWindow => windows::maximize_window(browser, arguments).await,
        Action::MinimizeWindow => windows::minimize_window(browser, arguments).await,
        Action::SetWindowSize => windows::set_window_size(browser, arguments).await,
        Action::CloseSession | Action::Downloads | Action::SaveDownload => {
            unreachable!("browser file and lifecycle operations use their owner")
        }
    }
}
