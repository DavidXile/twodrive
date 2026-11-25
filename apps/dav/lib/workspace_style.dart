import 'package:flutter/material.dart';

const canvasColor = Color(0xffedf0fa);
const panelColor = Color(0xfffafaff);
const railColor = Color(0xffe1e5f1);
const insetColor = Color(0xffeaeefb);
const accentColor = Color(0xff236ed5);
const inkColor = Color(0xff242a35);
const mutedColor = Color(0xff707987);
const successColor = Color(0xff328352);

class Surface extends StatelessWidget {
  const Surface({
    super.key,
    required this.child,
    this.padding = const EdgeInsets.all(22),
    this.color = panelColor,
    this.radius = 26,
    this.clip = false,
  });
  final Widget child;
  final EdgeInsetsGeometry padding;
  final Color color;
  final double radius;
  final bool clip;
  @override
  Widget build(BuildContext context) => Container(
    decoration: BoxDecoration(
      color: color,
      borderRadius: BorderRadius.circular(radius),
      boxShadow: const [
        BoxShadow(
          color: Color(0x070d245b),
          blurRadius: 22,
          offset: Offset(0, 7),
        ),
        BoxShadow(
          color: Color(0x040d245b),
          blurRadius: 3,
          offset: Offset(0, 2),
        ),
      ],
    ),
    clipBehavior: clip ? Clip.antiAlias : Clip.none,
    child: Padding(padding: padding, child: child),
  );
}

Color providerColor(String? kind) => switch (kind) {
  'google' => const Color(0xff48a282),
  'icloud' => const Color(0xff647087),
  'baidu' => const Color(0xffed737d),
  _ => accentColor,
};
IconData providerIcon(String? kind) => switch (kind) {
  'google' => Icons.add_to_drive,
  'icloud' => Icons.apple,
  'baidu' => Icons.pets_rounded,
  'peer' => Icons.devices_outlined,
  _ => Icons.cloud_rounded,
};
