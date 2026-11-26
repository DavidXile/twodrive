#ifndef TWODRIVE_STATUS_TRAY_H_
#define TWODRIVE_STATUS_TRAY_H_
#include <windows.h>
#include <shellapi.h>
#include <flutter/method_channel.h>
#include <flutter/encodable_value.h>
#include <memory>
#include <optional>

class StatusTray {
 public:
  StatusTray(HWND window, flutter::BinaryMessenger* messenger);
  ~StatusTray();
  std::optional<LRESULT> Message(UINT message, WPARAM wparam, LPARAM lparam);
 private:
  void Activate();
  void Dismiss();
  void Trace(const char* event);
  HWND window_;
  NOTIFYICONDATAW icon_{};
  HICON image_ = nullptr;
  UINT taskbar_created_ = 0;
  bool added_ = false;
  bool panel_ = false;
  bool focused_ = false;
  ULONGLONG dismissed_ = 0;
  POINT anchor_{};
  std::unique_ptr<flutter::MethodChannel<flutter::EncodableValue>> channel_;
};
#endif
