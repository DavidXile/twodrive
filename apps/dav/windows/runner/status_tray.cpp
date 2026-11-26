#include "status_tray.h"
#include <flutter/standard_method_codec.h>
#include <algorithm>
#include <cstdio>
#include <string>

namespace {
constexpr UINT kTrayMessage = WM_APP + 41;
std::wstring Wide(const std::string& value) {
  int size = MultiByteToWideChar(CP_UTF8, 0, value.data(), static_cast<int>(value.size()), nullptr, 0);
  std::wstring result(size, L'\0');
  MultiByteToWideChar(CP_UTF8, 0, value.data(), static_cast<int>(value.size()), result.data(), size);
  return result;
}
}
StatusTray::StatusTray(HWND window, flutter::BinaryMessenger* messenger) : window_(window) {
  icon_.cbSize = sizeof(icon_);
  icon_.hWnd = window;
  icon_.uID = 1;
  icon_.uFlags = NIF_MESSAGE | NIF_ICON | NIF_TIP;
  icon_.uCallbackMessage = kTrayMessage;
  wcscpy_s(icon_.szTip, L"TwoDrive Development");
  taskbar_created_ = RegisterWindowMessageW(L"TaskbarCreated");
  channel_ = std::make_unique<flutter::MethodChannel<flutter::EncodableValue>>(
      messenger, "twodrive/status_tray", &flutter::StandardMethodCodec::GetInstance());
  channel_->SetMethodCallHandler([this](const auto& call, auto result) {
    Trace(call.method_name().c_str());
    using flutter::EncodableValue;
    using flutter::EncodableMap;
    const auto* args = std::get_if<EncodableMap>(call.arguments());
    if (call.method_name() == "configure") {
      if (!args) { result->Error("argument", "icon required"); return; }
      const auto it = args->find(EncodableValue("icon"));
      if (it == args->end() || !std::holds_alternative<std::string>(it->second)) {
        result->Error("argument", "icon required"); return;
      }
      const auto title = args->find(EncodableValue("title"));
      const auto tip = Wide(title != args->end() && std::holds_alternative<std::string>(title->second)
          ? std::get<std::string>(title->second) : "TwoDrive Development");
      wcsncpy_s(icon_.szTip, tip.c_str(), _TRUNCATE);
      auto file = Wide(std::get<std::string>(it->second));
      HICON next = static_cast<HICON>(LoadImageW(nullptr, file.c_str(), IMAGE_ICON,
          GetSystemMetrics(SM_CXSMICON), GetSystemMetrics(SM_CYSMICON), LR_LOADFROMFILE));
      if (!next) { result->Error("icon", "cannot load tray icon"); return; }
      icon_.hIcon = next;
      if (!Shell_NotifyIconW(added_ ? NIM_MODIFY : NIM_ADD, &icon_)) {
        DestroyIcon(next); result->Error("tray", "cannot register tray icon"); return;
      }
      added_ = true;
      SetPropW(window_, L"TwoDrive.TrayReady", reinterpret_cast<HANDLE>(1));
      icon_.uVersion = NOTIFYICON_VERSION_4;
      Shell_NotifyIconW(NIM_SETVERSION, &icon_);
      if (image_) DestroyIcon(image_);
      image_ = next;
    } else if (call.method_name() == "panel") {
      if (!args) { result->Error("argument", "panel required"); return; }
      auto it = args->find(EncodableValue("enabled"));
      if (it == args->end() || !std::holds_alternative<bool>(it->second)) {
        result->Error("argument", "panel required"); return;
      }
      panel_ = std::get<bool>(it->second);
      focused_ = false;
      LONG_PTR style = GetWindowLongPtrW(window_, GWL_STYLE);
      if (panel_) {
        MONITORINFO monitor{sizeof(MONITORINFO)};
        GetMonitorInfoW(MonitorFromPoint(anchor_, MONITOR_DEFAULTTONEAREST), &monitor);
        const int dpi = static_cast<int>(GetDpiForWindow(window_));
        const int gap = MulDiv(8, dpi, 96);
        const int width = std::min(MulDiv(390, dpi, 96), static_cast<int>(monitor.rcWork.right - monitor.rcWork.left) - 2 * gap);
        const int height = std::min(MulDiv(570, dpi, 96), static_cast<int>(monitor.rcWork.bottom - monitor.rcWork.top) - 2 * gap);
        const int left = std::clamp(static_cast<int>(anchor_.x) - width / 2,
            static_cast<int>(monitor.rcWork.left) + gap, static_cast<int>(monitor.rcWork.right) - width - gap);
        const int top = anchor_.y < monitor.rcWork.top
            ? static_cast<int>(monitor.rcWork.top) + gap
            : static_cast<int>(monitor.rcWork.bottom) - height - gap;
        SetWindowLongPtrW(window_, GWL_STYLE, (style & ~WS_OVERLAPPEDWINDOW) | WS_POPUP);
        SetWindowPos(window_, HWND_TOPMOST, left, top, width, height, SWP_FRAMECHANGED | SWP_SHOWWINDOW);
        SetForegroundWindow(window_);
        // An already active window need not receive another WM_ACTIVATE.
        focused_ = GetForegroundWindow() == window_;
        Trace("panel shown");
      } else {
        SetWindowLongPtrW(window_, GWL_STYLE, (style & ~WS_POPUP) | WS_OVERLAPPEDWINDOW);
        SetWindowPos(window_, HWND_NOTOPMOST, 0, 0, 0, 0, SWP_FRAMECHANGED | SWP_NOMOVE | SWP_NOSIZE);
      }
    } else if (call.method_name() == "destroy") {
      if (added_) Shell_NotifyIconW(NIM_DELETE, &icon_);
      added_ = false;
    } else { result->NotImplemented(); return; }
    result->Success();
  });
}
StatusTray::~StatusTray() {
  RemovePropW(window_, L"TwoDrive.TrayReady");
  if (added_) Shell_NotifyIconW(NIM_DELETE, &icon_);
  if (image_) DestroyIcon(image_);
}
void StatusTray::Trace(const char* event) {
  wchar_t path[32768];
  DWORD length = GetEnvironmentVariableW(L"TWODRIVE_TRAY_TRACE_FILE", path, 32768);
  if (length == 0 || length >= 32768) return;
  FILE* file = nullptr;
  if (_wfopen_s(&file, path, L"a") != 0 || !file) return;
  fprintf(file, "%llu %s panel=%d visible=%d focused=%d error=%lu\n",
      GetTickCount64(), event, panel_, IsWindowVisible(window_), focused_, GetLastError());
  fclose(file);
}
void StatusTray::Dismiss() {
  Trace("dismiss requested");
  if (panel_ && IsWindowVisible(window_)) {
    dismissed_ = GetTickCount64();
    ShowWindow(window_, SW_HIDE);
  }
}
void StatusTray::Activate() {
  Trace("activate");
  if (panel_ && (IsWindowVisible(window_) || GetTickCount64() - dismissed_ < 250)) {
    Dismiss(); return;
  }
  NOTIFYICONIDENTIFIER id{sizeof(NOTIFYICONIDENTIFIER)};
  id.hWnd = window_; id.uID = 1;
  RECT bounds{};
  if (SUCCEEDED(Shell_NotifyIconGetRect(&id, &bounds))) {
    anchor_ = {(bounds.left + bounds.right) / 2, (bounds.top + bounds.bottom) / 2};
  } else GetCursorPos(&anchor_);
  flutter::EncodableMap args{{flutter::EncodableValue("x"), flutter::EncodableValue(static_cast<int>(anchor_.x))},
                            {flutter::EncodableValue("y"), flutter::EncodableValue(static_cast<int>(anchor_.y))}};
  channel_->InvokeMethod("activate", std::make_unique<flutter::EncodableValue>(args));
}
std::optional<LRESULT> StatusTray::Message(UINT message, WPARAM wparam, LPARAM lparam) {
  if (message == taskbar_created_ && added_) {
    Shell_NotifyIconW(NIM_ADD, &icon_); Shell_NotifyIconW(NIM_SETVERSION, &icon_);
  }
  if (message == kTrayMessage) {
    const auto event = LOWORD(lparam);
    if (event == NIN_SELECT || event == NIN_KEYSELECT || event == WM_CONTEXTMENU) Activate();
    return 0;
  }
  if (message == WM_ACTIVATE) {
    Trace(LOWORD(wparam) == WA_INACTIVE ? "focus out" : "focus in");
    if (LOWORD(wparam) == WA_INACTIVE) { if (focused_) Dismiss(); focused_ = false; }
    else focused_ = true;
  }
  if (message == WM_KEYDOWN && wparam == VK_ESCAPE && panel_) { Dismiss(); return 0; }
  return std::nullopt;
}
