import 'package:flutter/material.dart';

/// A single-choice sheet row with a trailing radio mark.
class ChoiceRow extends StatelessWidget {
  const ChoiceRow({
    super.key,
    required this.label,
    required this.selected,
    required this.onTap,
    this.leading,
    this.hint = false,
  });

  final String label;
  final bool selected;
  final VoidCallback onTap;
  final Widget? leading;

  /// Marks an inherited choice without presenting it as selected.
  final bool hint;

  @override
  Widget build(BuildContext context) {
    final colors = Theme.of(context).colorScheme;
    final marked = selected || hint;
    return Semantics(
      selected: selected,
      child: Material(
        color: selected
            ? colors.onSurface.withValues(alpha: .07)
            : Colors.transparent,
        borderRadius: BorderRadius.circular(12),
        clipBehavior: Clip.antiAlias,
        child: InkWell(
          onTap: onTap,
          child: Padding(
            padding: const EdgeInsets.symmetric(horizontal: 10, vertical: 11),
            child: Row(
              children: [
                if (leading case final leading?) ...[
                  leading,
                  const SizedBox(width: 10),
                ],
                Expanded(
                  child: Text(
                    label,
                    maxLines: 1,
                    overflow: TextOverflow.ellipsis,
                    style: TextStyle(fontSize: 15, color: colors.onSurface),
                  ),
                ),
                const SizedBox(width: 12),
                Container(
                  width: 18,
                  height: 18,
                  decoration: BoxDecoration(
                    shape: BoxShape.circle,
                    border: Border.all(
                      color: selected
                          ? colors.primary
                          : hint
                          ? colors.primary.withValues(alpha: .4)
                          : colors.onSurfaceVariant.withValues(alpha: .5),
                      width: marked ? 5 : 1.5,
                    ),
                  ),
                ),
              ],
            ),
          ),
        ),
      ),
    );
  }
}
