import 'package:flutter/material.dart';
import 'package:fluttertoast/fluttertoast.dart';

import 'kit.dart';

/// Transient feedback shares one queue, without covering navigation or input.
void showToast(
  BuildContext context,
  String message, {
  String? icon,
  Key? key,
  Duration duration = const Duration(seconds: 2),
  String? action,
  VoidCallback? onAction,
}) {
  if (!context.mounted) return;
  final toast = FToast()..removeQueuedCustomToasts();
  toast.init(context);
  toast.showToast(
    ignorePointer: onAction == null,
    toastDuration: duration,
    fadeDuration: const Duration(milliseconds: 180),
    positionedToastBuilder: (context, child, _) => Positioned(
      left: 24,
      right: 24,
      bottom:
          MediaQuery.viewInsetsOf(context).bottom +
          MediaQuery.paddingOf(context).bottom +
          88,
      child: Center(child: child),
    ),
    child: Semantics(
      liveRegion: true,
      child: Surface(
        key: key ?? const ValueKey('app-toast'),
        radius: 28,
        padding: const EdgeInsets.symmetric(horizontal: 18, vertical: 12),
        child: Column(
          mainAxisSize: MainAxisSize.min,
          children: [
            Row(
              mainAxisSize: MainAxisSize.min,
              children: [
                if (icon != null) ...[
                  AppIcon(icon, size: 18),
                  const SizedBox(width: 10),
                ],
                Flexible(child: Text(message)),
              ],
            ),
            if (action != null && onAction != null) ...[
              const SizedBox(height: 4),
              TextButton(
                onPressed: () {
                  toast.removeCustomToast();
                  onAction();
                },
                child: Text(action),
              ),
            ],
          ],
        ),
      ),
    ),
  );
}
