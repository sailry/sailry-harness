import 'package:flutter/material.dart';

import 'theme.dart';

/// The platform's switch at four fifths of its size, laid out at that size:
/// the stock one reads heavy beside the app's 15-point rows. Neither theme
/// can size a switch, so it is scaled here, once for every row.
class AppSwitch extends StatelessWidget {
  const AppSwitch({super.key, required this.value, required this.onChanged});

  final bool value;
  final ValueChanged<bool>? onChanged;

  @override
  Widget build(BuildContext context) => SizedBox(
    width: 42,
    height: 26,
    child: FittedBox(
      child: Switch.adaptive(
        value: value,
        onChanged: onChanged,
        // On in the chosen brand color, as the app's other selections.
        activeTrackColor: SailryTheme.brand(context).color,
        materialTapTargetSize: MaterialTapTargetSize.shrinkWrap,
      ),
    ),
  );
}

/// A list row ending in an [AppSwitch]: tapping anywhere on it flips the
/// value, as SwitchListTile does.
class SwitchTile extends StatelessWidget {
  const SwitchTile({
    super.key,
    required this.title,
    required this.value,
    required this.onChanged,
    this.leading,
    this.contentPadding,
  });

  final Widget title;
  final bool value;
  final ValueChanged<bool>? onChanged;
  final Widget? leading;
  final EdgeInsetsGeometry? contentPadding;

  @override
  Widget build(BuildContext context) => MergeSemantics(
    child: ListTile(
      contentPadding: contentPadding,
      leading: leading,
      title: title,
      enabled: onChanged != null,
      trailing: AppSwitch(value: value, onChanged: onChanged),
      onTap: onChanged == null ? null : () => onChanged!(!value),
    ),
  );
}
