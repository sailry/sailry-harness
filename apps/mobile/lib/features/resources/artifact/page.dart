import 'dart:async';
import 'dart:convert';
import 'dart:typed_data';

import 'package:flutter/foundation.dart' show ValueListenable;
import 'package:flutter/material.dart';

import '../../../content/code.dart';
import '../../../content/transfers.dart';
import '../../../l10n/strings.dart';
import '../../../runtime/json.dart';
import '../../../runtime/session.dart';
import '../../../ui/kit.dart';
import '../../../ui/toast.dart';
import 'content.dart';
import 'submission.dart';
import 'surface.dart';

class ArtifactPage extends StatefulWidget {
  const ArtifactPage({
    super.key,
    required this.host,
    required this.artifact,
    this.session,
    this.actions,
    this.changes,
  });
  final HostConnection host;
  final Map<String, dynamic> artifact;
  final String? session;
  final ArtifactActions? actions;
  final ValueListenable<Map<String, dynamic>>? changes;

  @override
  State<ArtifactPage> createState() => _ArtifactPageState();
}

class _ArtifactPageState extends State<ArtifactPage> {
  var _surface = GlobalKey<ArtifactSurfaceState>();
  Map<String, dynamic>? _content;
  List<Map<String, dynamic>> _revisions = [];
  int? _before;
  bool _loading = true;
  bool _listing = false;
  bool _exporting = false;
  bool _rendered = false;
  bool _following = true;
  int? _selectedRevision;
  bool _dirty = false;
  bool _invalidated = false;
  int _generation = 0;
  Timer? _refresh;
  Object? _cursor;
  late bool _connected;

  Map<String, dynamic> get _artifact =>
      _content == null ? widget.artifact : object(_content!['artifact']);

  String get _session => widget.session ?? text(widget.artifact['session']);
  bool get _inherited => _session != widget.artifact['session'];

  @override
  void initState() {
    super.initState();
    _connected = widget.host.connected;
    widget.host.addListener(_hostChanged);
    widget.changes?.addListener(_changed);
    _cursor = widget.changes?.value['artifact_cursor'];
    if (_inherited) {
      _following = false;
      _selectedRevision = widget.artifact['revision'] as int;
    }
    unawaited(_load(_selectedRevision));
    unawaited(_list());
  }

  void _hostChanged() {
    if (mounted) setState(() {});
    final connected = widget.host.connected;
    if (connected && !_connected) {
      if (_content == null ||
          (_selectedRevision != null &&
              _selectedRevision != _artifact['revision'])) {
        unawaited(_load(_following ? null : _selectedRevision));
      }
      unawaited(_list());
    }
    _connected = connected;
  }

  void _changed() {
    final cursor = widget.changes?.value['artifact_cursor'];
    if (cursor == _cursor) return;
    _cursor = cursor;
    _refresh?.cancel();
    // Artifact publication invalidates metadata even when this document is outside
    // the bounded artifact snapshot. No controller replays artifact changes.
    _refresh = Timer(const Duration(milliseconds: 400), () {
      if (mounted && widget.host.connected) unawaited(_list());
    });
  }

  Future<void> _load(int? revision, {bool automatic = false}) async {
    if (!widget.host.connected) return;
    final generation = ++_generation;
    setState(() => _loading = true);
    var applied = false;
    try {
      final response = await widget.host.command('read_artifact', {
        'id': widget.artifact['id'],
        'revision': revision,
        'session': _session,
      });
      final content = object(response['data']);
      final artifact = object(content['artifact']);
      if (artifact['id'] != widget.artifact['id'] ||
          artifact['session'] != widget.artifact['session'] ||
          content['latest_revision'] is! int ||
          (content['latest_revision'] as int) < (artifact['revision'] as int) ||
          (revision != null && artifact['revision'] != revision) ||
          utf8.encode(text(content['html'])).length > artifactLimit) {
        throw const FormatException(
          'Artifact response does not match its capture',
        );
      }
      if (!mounted || generation != _generation) return;
      if (automatic && (_dirty || widget.actions?.pending() == true)) {
        setState(() {
          _following = false;
          if (_content != null) {
            _content!['latest_revision'] = content['latest_revision'];
          }
        });
        return;
      }
      setState(() {
        _surface = GlobalKey<ArtifactSurfaceState>();
        _content = content;
        _rendered = false;
        _dirty = false;
      });
      applied = true;
    } catch (_) {
      if (mounted && generation == _generation) {
        showToast(context, context.tr('resourcePreviewFailed'));
      }
    } finally {
      if (mounted && generation == _generation) {
        setState(() => _loading = false);
        if (applied) _follow();
      }
    }
  }

  Future<void> _list({bool older = false}) async {
    if (!widget.host.connected) return;
    if (_listing) {
      if (!older) _invalidated = true;
      return;
    }
    _listing = true;
    try {
      final response = await widget.host.command('list_artifact_revisions', {
        'id': widget.artifact['id'],
        'session': _session,
        'before': older ? _before : null,
        'limit': 64,
      });
      final page = object(response['data']);
      final items = objects(page['items']);
      if (items.length > 64 ||
          items.any((item) => item['id'] != widget.artifact['id'])) {
        throw const FormatException(
          'Artifact revisions do not match their capture',
        );
      }
      if (!mounted) return;
      setState(() {
        _revisions = older ? [..._revisions, ...items] : items;
        _before = page['next_before'] as int?;
        if (_content != null && _revisions.isNotEmpty) {
          final latest = _revisions.first['revision'] as int;
          if (latest > (_content!['latest_revision'] as int)) {
            _content!['latest_revision'] = latest;
          }
        }
      });
    } catch (_) {
      if (mounted) showToast(context, context.tr('resourcePreviewFailed'));
    } finally {
      _listing = false;
      if (mounted) {
        _follow();
        if (_invalidated) {
          _invalidated = false;
          unawaited(_list());
        }
      }
    }
  }

  void _follow() {
    if (!mounted ||
        !_following ||
        _loading ||
        _content == null ||
        _revisions.isEmpty) {
      return;
    }
    final latest = _revisions.first['revision'] as int;
    if (latest <= (_artifact['revision'] as int)) return;
    if (_dirty || widget.actions?.pending() == true) {
      setState(() => _following = false);
    } else {
      unawaited(_load(latest, automatic: true));
    }
  }

  Future<void> _versions() => showAppSheet(
    context,
    context.tr('artifactVersions'),
    child: StatefulBuilder(
      builder: (context, update) => Column(
        mainAxisSize: MainAxisSize.min,
        children: [
          for (final revision in _revisions)
            ListTile(
              key: ValueKey('artifact-version-${revision['revision']}'),
              title: Text('v${revision['revision']}'),
              subtitle: Text(text(revision['title'])),
              selected: revision['revision'] == _artifact['revision'],
              onTap: () {
                Navigator.pop(context);
                _following = false;
                _selectedRevision = revision['revision'] as int;
                unawaited(_load(_selectedRevision));
              },
            ),
          if (_before != null)
            TextButton(
              onPressed: _listing
                  ? null
                  : () async {
                      await _list(older: true);
                      if (context.mounted) update(() {});
                    },
              child: Text(context.tr('conversationOlder')),
            ),
        ],
      ),
    ),
  );

  Future<void> _save() async {
    if (_exporting || !_rendered || _surface.currentState == null) return;
    final artifact = Map<String, dynamic>.of(_artifact);
    setState(() => _exporting = true);
    try {
      final html = await _surface.currentState!.exportHtml();
      if (mounted) {
        final title = text(
          artifact['title'],
        ).replaceAll(RegExp(r'[/\\:*?"<>|]'), '_');
        await saveContent(
          Uint8List.fromList(utf8.encode(html)),
          '$title-v${artifact['revision']}.html',
          'text/html',
        );
      }
    } catch (_) {
      if (mounted) showToast(context, context.tr('fileSaveFailed'));
    } finally {
      if (mounted) setState(() => _exporting = false);
    }
  }

  @override
  void dispose() {
    _generation++;
    _refresh?.cancel();
    widget.host.removeListener(_hostChanged);
    widget.changes?.removeListener(_changed);
    super.dispose();
  }

  @override
  Widget build(BuildContext context) => PageFrame(
    title: text(_artifact['title']),
    scroll: false,
    padding: EdgeInsets.zero,
    loading: _loading,
    actions: [
      TextButton(
        onPressed: _loading || _revisions.isEmpty ? null : _versions,
        child: Text('v${_artifact['revision']}'),
      ),
      RoundButton(
        icon: 'refresh',
        tooltip: context.tr('refresh'),
        onPressed: _loading || !widget.host.connected
            ? null
            : () {
                unawaited(_load(_artifact['revision'] as int));
                unawaited(_list());
              },
      ),
      RoundButton(
        icon: 'arrow-down',
        tooltip: context.tr('save'),
        onPressed: !_rendered || _exporting ? null : _save,
      ),
    ],
    child: _content == null
        ? const SizedBox.expand()
        : _content!['view'] == null
        ? SingleChildScrollView(
            child: CodeBlock(text(_content!['html']), language: 'html'),
          )
        : ArtifactSurface(
            key: _surface,
            content: _content!,
            connected: widget.host.connected,
            actions: _inherited ? null : widget.actions,
            readonly: _artifact['revision'] != _content!['latest_revision'],
            onEdited: () => _dirty = true,
            onReady: () {
              if (mounted) setState(() => _rendered = true);
            },
          ),
  );
}
