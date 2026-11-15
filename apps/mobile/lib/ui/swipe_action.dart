import 'package:flutter/material.dart';
import 'package:flutter_slidable/flutter_slidable.dart';

/// An action revealed by swiping a row: a rounded tile standing apart from
/// the row and from its neighbor, rather than SlidableAction's flush block.
/// A null [onPressed] shows it dimmed and inert.
class SwipeAction extends StatelessWidget {
  const SwipeAction({
    super.key,
    required this.icon,
    required this.label,
    required this.color,
    required this.foreground,
    required this.onPressed,
  });

  final IconData icon;
  final String label;
  final Color color;
  final Color foreground;
  final SlidableActionCallback? onPressed;

  @override
  Widget build(BuildContext context) => CustomSlidableAction(
    onPressed: onPressed,
    backgroundColor: Colors.transparent,
    foregroundColor: foreground,
    child: Opacity(
      opacity: onPressed == null ? .5 : 1,
      child: Container(
        margin: const EdgeInsets.fromLTRB(8, 6, 0, 6),
        decoration: BoxDecoration(
          color: color,
          borderRadius: BorderRadius.circular(14),
        ),
        alignment: Alignment.center,
        child: Column(
          mainAxisSize: MainAxisSize.min,
          spacing: 4,
          children: [
            Icon(icon, size: 20, color: foreground),
            Text(
              label,
              maxLines: 1,
              overflow: TextOverflow.ellipsis,
              style: TextStyle(fontSize: 12.5, color: foreground),
            ),
          ],
        ),
      ),
    ),
  );
}
