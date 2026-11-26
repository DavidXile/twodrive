import 'package:flutter/foundation.dart';
import 'package:flutter/material.dart';
import 'workspace_style.dart';

/// Use one family for Latin and CJK text, including Material component labels.
/// These are system fonts; no account data or downloaded font is required.
ThemeData appTheme({TargetPlatform? platform}) {
  final target = platform ?? defaultTargetPlatform;
  final family = switch (target) {
    TargetPlatform.windows => 'Microsoft YaHei UI',
    TargetPlatform.macOS || TargetPlatform.iOS => 'PingFang SC',
    TargetPlatform.linux => 'TwoDriveSans',
    _ => 'Noto Sans CJK SC',
  };
  return ThemeData(
    platform: target,
    useMaterial3: true,
    fontFamily: family,
    fontFamilyFallback: target == TargetPlatform.linux
        ? const ['TwoDriveCJK']
        : const [
            'Microsoft YaHei',
            'PingFang SC',
            'Noto Sans CJK SC',
            'Noto Sans',
            'sans-serif',
          ],
    scaffoldBackgroundColor: canvasColor,
    colorScheme: ColorScheme.fromSeed(seedColor: accentColor).copyWith(
      primary: accentColor,
      surface: panelColor,
      onSurface: inkColor,
      outline: const Color(0xffd8deec),
      surfaceContainerHighest: insetColor,
    ),
    textTheme: const TextTheme(
      bodyMedium: TextStyle(fontSize: 14, color: inkColor),
      bodySmall: TextStyle(fontSize: 12, color: mutedColor),
      titleMedium: TextStyle(fontSize: 16, fontWeight: FontWeight.w600),
    ),
    dividerColor: const Color(0xffe4e8f2),
    iconTheme: const IconThemeData(size: 20, color: Color(0xff626c7b)),
    tooltipTheme: TooltipThemeData(
      decoration: BoxDecoration(
        color: const Color(0xff34425d),
        borderRadius: BorderRadius.circular(9),
      ),
    ),
    filledButtonTheme: FilledButtonThemeData(
      style: FilledButton.styleFrom(
        minimumSize: const Size(0, 42),
        padding: const EdgeInsets.symmetric(horizontal: 20, vertical: 12),
        shape: RoundedRectangleBorder(borderRadius: BorderRadius.circular(14)),
      ),
    ),
    outlinedButtonTheme: OutlinedButtonThemeData(
      style: OutlinedButton.styleFrom(
        minimumSize: const Size(0, 42),
        side: const BorderSide(color: Color(0xffdce2ef)),
        shape: RoundedRectangleBorder(borderRadius: BorderRadius.circular(14)),
      ),
    ),
    inputDecorationTheme: InputDecorationTheme(
      filled: true,
      fillColor: const Color(0xfff0f2fb),
      isDense: true,
      contentPadding: const EdgeInsets.symmetric(horizontal: 16, vertical: 16),
      border: OutlineInputBorder(
        borderSide: BorderSide.none,
        borderRadius: BorderRadius.circular(13),
      ),
      enabledBorder: OutlineInputBorder(
        borderSide: BorderSide.none,
        borderRadius: BorderRadius.circular(13),
      ),
      focusedBorder: OutlineInputBorder(
        borderSide: const BorderSide(color: accentColor, width: 1.3),
        borderRadius: BorderRadius.circular(13),
      ),
    ),
    switchTheme: SwitchThemeData(
      thumbColor: const WidgetStatePropertyAll(Colors.white),
      trackOutlineColor: const WidgetStatePropertyAll(Colors.transparent),
      trackColor: WidgetStateProperty.resolveWith(
        (s) => s.contains(WidgetState.selected)
            ? accentColor
            : const Color(0xffdce1ed),
      ),
    ),
    cardTheme: CardThemeData(
      color: panelColor,
      surfaceTintColor: Colors.transparent,
      elevation: 2,
      shadowColor: const Color(0x160d245b),
      margin: const EdgeInsets.only(bottom: 14),
      shape: RoundedRectangleBorder(borderRadius: BorderRadius.circular(26)),
    ),
  );
}
