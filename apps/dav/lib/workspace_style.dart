import 'package:flutter/material.dart';

const canvasColor = Color(0xffedf0fa);
const panelColor = Color(0xfffafaff);
const railColor = Color(0xffe1e5f1);
const insetColor = Color(0xffeaeefb);
const accentColor = Color(0xff236ed5);
const inkColor = Color(0xff242a35);
const mutedColor = Color(0xff707987);
const successColor = Color(0xff328352);

/// Reserve more room per column when desktop accessibility enlarges the text.
bool fitsScaledWidth(BuildContext context, double width, double minimum) {
  final scale = MediaQuery.textScalerOf(context).scale(16) / 16;
  return width >= minimum * scale.clamp(1, double.infinity);
}

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

/// A workspace selector whose label and hint grow with the system text size.
class WorkspaceSelect<T> extends StatelessWidget {
  const WorkspaceSelect({
    super.key,
    required this.fieldKey,
    required this.label,
    required this.value,
    required this.items,
    required this.onChanged,
    this.hint,
    this.helperText,
    this.selectedItemBuilder,
  });

  final Key fieldKey;
  final String label;
  final T? value;
  final List<DropdownMenuItem<T>> items;
  final ValueChanged<T?>? onChanged;
  final Widget? hint;
  final String? helperText;
  final DropdownButtonBuilder? selectedItemBuilder;

  @override
  Widget build(BuildContext context) => Surface(
    color: Colors.white,
    radius: 20,
    padding: const EdgeInsets.fromLTRB(16, 12, 16, 8),
    child: Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        ExcludeSemantics(child: Text(label)),
        Semantics(
          label: label,
          child: DropdownButtonHideUnderline(
            child: DropdownButton<T>(
              key: fieldKey,
              isExpanded: true,
              itemHeight: null,
              dropdownColor: Colors.white,
              borderRadius: BorderRadius.circular(14),
              menuMaxHeight: 360,
              value: value,
              hint: hint,
              items: items,
              selectedItemBuilder: selectedItemBuilder,
              onChanged: onChanged,
            ),
          ),
        ),
        if (helperText case final text?)
          Padding(
            padding: const EdgeInsets.only(bottom: 4),
            child: Text(
              text,
              style: Theme.of(
                context,
              ).textTheme.bodySmall?.copyWith(color: mutedColor),
            ),
          ),
      ],
    ),
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
