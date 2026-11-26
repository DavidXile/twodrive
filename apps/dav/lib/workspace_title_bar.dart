import 'dart:async';

import 'package:flutter/material.dart';
import 'package:window_manager/window_manager.dart';

import 'language.dart';
import 'workspace_style.dart';

class WorkspaceTitleBar extends StatefulWidget implements PreferredSizeWidget {
  const WorkspaceTitleBar({super.key, required this.sidebarWidth});

  final double sidebarWidth;
  static const height = 36.0;

  @override
  Size get preferredSize => const Size.fromHeight(height);

  @override
  State<WorkspaceTitleBar> createState() => _WorkspaceTitleBarState();
}

class _WorkspaceTitleBarState extends State<WorkspaceTitleBar>
    with WindowListener {
  var maximized = false;

  @override
  void initState() {
    super.initState();
    windowManager.addListener(this);
    unawaited(readMaximized());
  }

  Future<void> readMaximized() async {
    final value = await windowManager.isMaximized();
    if (mounted && value != maximized) setState(() => maximized = value);
  }

  @override
  void onWindowMaximize() => setState(() => maximized = true);

  @override
  void onWindowUnmaximize() => setState(() => maximized = false);

  @override
  void dispose() {
    windowManager.removeListener(this);
    super.dispose();
  }

  @override
  Widget build(BuildContext context) => Material(
    color: canvasColor,
    child: SizedBox(
      height: WorkspaceTitleBar.height,
      child: Row(
        children: [
          Expanded(
            child: DragToMoveArea(
              child: Align(
                alignment: Alignment.centerLeft,
                child: SizedBox(
                  width: widget.sidebarWidth,
                  height: double.infinity,
                  child: const ColoredBox(color: railColor),
                ),
              ),
            ),
          ),
          button(
            icon: Icons.remove_rounded,
            label: uiText('最小化', 'Minimize'),
            action: windowManager.minimize,
          ),
          button(
            icon: maximized ? Icons.filter_none_rounded : Icons.crop_square,
            label: maximized
                ? uiText('还原窗口', 'Restore window')
                : uiText('最大化', 'Maximize'),
            action: maximized
                ? windowManager.unmaximize
                : windowManager.maximize,
          ),
          button(
            icon: Icons.close_rounded,
            label: uiText('关闭窗口', 'Close window'),
            action: windowManager.close,
            close: true,
          ),
        ],
      ),
    ),
  );

  Widget button({
    required IconData icon,
    required String label,
    required Future<void> Function() action,
    bool close = false,
  }) => SizedBox(
    width: 46,
    height: WorkspaceTitleBar.height,
    child: IconButton(
      tooltip: label,
      icon: Icon(icon, size: 16),
      onPressed: () => unawaited(action()),
      style: ButtonStyle(
        shape: const WidgetStatePropertyAll(RoundedRectangleBorder()),
        foregroundColor: WidgetStateProperty.resolveWith((states) {
          return close && states.contains(WidgetState.hovered)
              ? Colors.white
              : inkColor;
        }),
        backgroundColor: WidgetStateProperty.resolveWith((states) {
          if (!states.contains(WidgetState.hovered)) return null;
          return close ? const Color(0xffc42b1c) : insetColor;
        }),
      ),
    ),
  );
}
