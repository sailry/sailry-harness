import 'dart:async';

import 'package:flutter/material.dart';
import 'package:url_launcher/url_launcher.dart';

import '../../l10n/strings.dart';
import '../../ui/kit.dart';
import '../../ui/toast.dart';
import 'releases.dart';
import 'service.dart';

Future<void> openUpdate(BuildContext context, Release release) async {
  try {
    if (await launchUrl(
      release.download,
      mode: LaunchMode.externalApplication,
    )) {
      return;
    }
  } catch (_) {
    // The platform may reject an otherwise valid download link.
  }
  if (context.mounted) showToast(context, context.tr('updatesOpenFailed'));
}

Future<void> checkUpdates(
  BuildContext context,
  AppUpdates updates, {
  bool automatic = false,
}) async {
  try {
    final result = await updates.check();
    if (!context.mounted || result == null) return;
    if (result == UpdateResult.available) {
      final release = updates.available!;
      showToast(
        context,
        context
            .tr('updatesAvailable')
            .replaceAll('{version}', '${release.version}'),
        key: const ValueKey('update-toast'),
        duration: const Duration(seconds: 10),
        action: context.tr('updatesDownload'),
        onAction: () => unawaited(openUpdate(context, release)),
      );
    } else if (!automatic) {
      showToast(
        context,
        context.tr(
          result == UpdateResult.current
              ? 'updatesCurrent'
              : 'updatesUnpublished',
        ),
      );
    }
  } catch (_) {
    if (context.mounted && !automatic) {
      showToast(context, context.tr('updatesCheckFailed'));
    }
  }
}

class UpdateDelivery extends StatefulWidget {
  const UpdateDelivery({super.key, required this.updates, required this.child});

  final AppUpdates updates;
  final Widget child;

  @override
  State<UpdateDelivery> createState() => _UpdateDeliveryState();
}

class _UpdateDeliveryState extends State<UpdateDelivery> {
  @override
  void initState() {
    super.initState();
    WidgetsBinding.instance.addPostFrameCallback((_) {
      if (mounted) {
        unawaited(checkUpdates(context, widget.updates, automatic: true));
      }
    });
  }

  @override
  Widget build(BuildContext context) => widget.child;
}

class UpdateSettings extends StatelessWidget {
  const UpdateSettings({super.key, this.updates});

  final AppUpdates? updates;

  @override
  Widget build(BuildContext context) {
    final service = updates;
    return service == null
        ? _rows(context)
        : ListenableBuilder(
            listenable: service,
            builder: (context, _) => _rows(context),
          );
  }

  Widget _rows(BuildContext context) {
    final service = updates;
    final release = service?.available;
    return Surface(
      padding: const EdgeInsets.symmetric(vertical: 4),
      child: Column(
        children: [
          if (service?.version != null) ...[
            ListTile(
              leading: const AppIcon('info'),
              title: Text(context.tr('updatesVersion')),
              trailing: Text(service!.version!),
            ),
            const Divider(height: 1, indent: 16, endIndent: 16),
          ],
          ListTile(
            key: const ValueKey('check-updates'),
            enabled: service != null && !service.checking,
            leading: const AppIcon('refresh'),
            title: Text(
              context.tr(
                service?.checking == true ? 'updatesChecking' : 'updatesCheck',
              ),
            ),
            trailing: service?.checking == true
                ? const SizedBox.square(
                    dimension: 18,
                    child: CircularProgressIndicator(strokeWidth: 2),
                  )
                : const AppIcon('chevron', size: 14),
            onTap: service == null || service.checking
                ? null
                : () => unawaited(checkUpdates(context, service)),
          ),
          if (release != null) ...[
            const Divider(height: 1, indent: 16, endIndent: 16),
            ListTile(
              key: const ValueKey('download-update'),
              leading: const AppIcon('arrow-down'),
              title: Text(context.tr('updatesDownload')),
              trailing: const AppIcon('chevron', size: 14),
              onTap: () => unawaited(openUpdate(context, release)),
            ),
          ],
        ],
      ),
    );
  }
}
