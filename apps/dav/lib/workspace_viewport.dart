import 'dart:math' as math;

import 'package:flutter/material.dart';

class WorkspaceViewport extends StatelessWidget {
  const WorkspaceViewport({super.key, required this.child});

  final Widget child;

  // Match the runner's initial 1280×720 window. Both dimensions must grow
  // before scaling so wide, shallow windows keep enough vertical space.
  static const reference = Size(1280, 720);

  @override
  Widget build(BuildContext context) => LayoutBuilder(
    builder: (context, constraints) {
      final window = constraints.biggest;
      final scale = math
          .min(window.width / reference.width, window.height / reference.height)
          .clamp(1.0, 1.4)
          .toDouble();
      final layout = window / scale;
      return SizedBox.fromSize(
        size: window,
        child: FittedBox(
          child: SizedBox.fromSize(
            size: layout,
            child: MediaQuery(
              data: MediaQuery.of(context).copyWith(size: layout),
              child: child,
            ),
          ),
        ),
      );
    },
  );
}
