import 'package:flutter/widgets.dart';

import '../../../content/links.dart';
import '../../../content/paths.dart';
import '../../../l10n/strings.dart';
import '../../../runtime/json.dart';
import '../../../runtime/notices.dart';
import '../../../runtime/session.dart';
import '../../../ui/toast.dart';
import '../../resources/contributions.dart';
import '../../resources/file_draft.dart';
import '../../resources/file_navigation.dart';

final _drivePath = RegExp(r'^[A-Za-z]:/');
final _drivePrefix = RegExp(r'^/?[A-Za-z]:');

class ConversationLinks {
  const ConversationLinks(
    this.host,
    this.worktree, {
    this.session,
    this.documents,
    this.openSession,
  });

  final HostConnection host;
  final String worktree;
  final String? session;
  final Map<String, FileDocument>? documents;
  final bool Function(String link)? openSession;

  Future<void> openUri(BuildContext context, Uri uri) async {
    final target = parseContentLink(uri.toString());
    if (target == null) {
      _unavailable(context);
      return;
    }
    if (isContentAnchor(target)) return;
    if (!target.hasScheme || target.scheme == 'file') {
      await _openPath(
        context,
        target.path.endsWith('/') ? 'directory' : 'file',
        target,
        lineReference: true,
        linkUri: target,
      );
      return;
    }
    await _open(
      context,
      uri: target,
      fallback: () async {
        if (target.scheme == 'sailry' &&
            openSession?.call(target.toString()) == true) {
          return;
        }
        if (await launchExternalContentLink(target)) return;
        if (context.mounted) _unavailable(context);
      },
    );
  }

  Future<void> openFile(BuildContext context, Object? path) =>
      _openLiteralPath(context, 'file', path);

  Future<void> openReference(
    BuildContext context,
    Map<String, dynamic> target, {
    required Future<void> Function() fallback,
  }) {
    final kind = text(target['kind']);
    final data = target['data'];
    if (kind == 'file' || kind == 'directory') {
      return _openLiteralPath(context, kind, data);
    }
    return _open(context, kind: kind, data: data, fallback: fallback);
  }

  Future<void> _openLiteralPath(
    BuildContext context,
    String kind,
    Object? path,
  ) async {
    if (path is! String) {
      _unavailable(context);
      return;
    }
    final normalized = path.replaceAll('\\', '/');
    if (normalized.startsWith('//')) {
      _unavailable(context);
      return;
    }
    await _openPath(
      context,
      kind,
      Uri(path: _drivePath.hasMatch(normalized) ? '/$normalized' : normalized),
    );
  }

  Future<void> _openPath(
    BuildContext context,
    String kind,
    Uri uri, {
    bool lineReference = false,
    Uri? linkUri,
  }) async {
    final tree = objects(
      host.snapshot['worktrees'],
    ).where((tree) => tree['id'] == worktree).firstOrNull;
    final root = text(tree?['path']);
    final path = resourcePath(
      uri,
      root.isEmpty ? null : root,
      allowRoot: kind == 'directory',
      lineReference: lineReference,
    );
    if (uri.authority.isNotEmpty ||
        path == null ||
        _drivePrefix.hasMatch(path) ||
        worktree.isEmpty ||
        !host.connected) {
      _unavailable(context);
      return;
    }
    await _open(
      context,
      kind: kind,
      data: path,
      uri: linkUri,
      fallback: () async {
        if (kind == 'file') {
          await openResourceFile(
            context,
            host,
            worktree,
            path,
            documents: documents,
          );
        } else {
          _unavailable(context);
        }
      },
    );
  }

  Future<void> _open(
    BuildContext context, {
    String? kind,
    Object? data,
    Uri? uri,
    required Future<void> Function() fallback,
  }) async {
    if (!context.mounted) return;
    final features =
        context
            .getInheritedWidgetOfExactType<ProjectResourcesScope>()
            ?.features ??
        const <ProjectResourceFeature>[];
    try {
      final entries = features.expand((feature) => feature.links);
      final contribution =
          (kind == null
              ? null
              : entries.where((entry) => kind == entry.kind).firstOrNull) ??
          (uri?.hasScheme == true
              ? entries
                    .where((entry) => uri!.scheme == entry.scheme)
                    .firstOrNull
              : null);
      if (contribution != null) {
        if (!host.connected && (uri == null || !isExternalContentLink(uri))) {
          _unavailable(context);
          return;
        }
        await contribution.open(context, (
          host: host,
          worktree: worktree,
          session: session,
          kind: kind,
          data: data,
          uri: uri,
          documents: documents,
        ));
        return;
      }
      if (kind == 'directory') {
        final legacy = features
            .map((feature) => feature.openDirectory)
            .whereType<OpenDirectory>()
            .firstOrNull;
        if (legacy != null) {
          await legacy(context, host, worktree, data as String);
          return;
        }
      }
      await fallback();
    } on CommandFailure catch (error) {
      if (context.mounted) showFailure(context, error);
    } catch (_) {
      if (context.mounted) _unavailable(context);
    }
  }
}

void _unavailable(BuildContext context) {
  if (context.mounted) showToast(context, context.tr('fileLinkUnavailable'));
}
