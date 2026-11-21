#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

fn main() -> virtual_avatar_connect::Result<()> {
	let result = virtual_avatar_connect::run_desktop();
	if let Err(error) = &result {
		// Release Windows binaries have no console. Keep the actual error visible
		// without requiring the failed WebView to render its own recovery screen.
		#[cfg(all(windows, not(feature = "desktop-native-review")))]
		show_failure(&format!("{error:#}"));
		#[cfg(any(not(windows), feature = "desktop-native-review"))]
		eprintln!("VAC desktop failed: {error:#}");
	}
	result
}

#[cfg(all(windows, not(feature = "desktop-native-review")))]
fn show_failure(detail: &str) {
	use windows::core::PCWSTR;
	use windows::Win32::UI::WindowsAndMessaging::{MessageBoxW, MB_ICONERROR, MB_OK};
	let mut chars = detail.chars();
	let mut detail: String = chars.by_ref().take(4000).map(|c| if c == '\0' { ' ' } else { c }).collect();
	if chars.next().is_some() { detail.push_str("\n（詳細が長いため、以降を省略）"); }
	let message = format!("VACを起動・継続できませんでした。\n\n{detail}\n\n配布フォルダーの START-HERE.txt に必要な実行環境と導入手順があります。\n設定ファイルの問題が表示された場合は、その内容を確認してから起動し直してください。");
	let text: Vec<u16> = message.encode_utf16().chain(Some(0)).collect();
	let title: Vec<u16> = "UN Virtual Avatar Connect — エラー".encode_utf16().chain(Some(0)).collect();
	// Both buffers remain live, NUL-terminated, throughout the synchronous call.
	unsafe { MessageBoxW(None, PCWSTR(text.as_ptr()), PCWSTR(title.as_ptr()), MB_OK | MB_ICONERROR); }
}
