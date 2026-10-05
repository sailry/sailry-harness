import 'package:flutter/material.dart';

import '../../content/markdown.dart';
import '../../content/transfers.dart';
import '../../l10n/strings.dart';
import '../../ui/kit.dart';
import '../../runtime/session.dart';
import 'live_files.dart';
import 'file_location.dart';
import 'workspace.dart';

class FilesPage extends StatefulWidget {
  const FilesPage({
    super.key,
    this.hostId,
    this.worktreeId,
    this.host = 'Studio',
    this.project = 'sailry-web',
    this.branch = 'feature/sign-in',
  });
  final String host;
  final String? hostId;
  final String? worktreeId;
  final String project;
  final String branch;

  @override
  State<FilesPage> createState() => _FilesPageState();
}

class _FilesPageState extends State<FilesPage> {
  String get _project => widget.project;
  String get _branch => widget.branch;
  String _directory = '';
  final Map<String, String> _edits = {};
  final Map<String, String> _drafts = {};

  static const _files = [
    'src/pages/Login.tsx',
    'src/styles/login.css',
    'src/tests/login.test.ts',
    'public/favicon.svg',
    'public/robots.txt',
    'package.json',
    'README.md',
    'vite.config.ts',
  ];

  String _key(String path) => '${widget.host}/$_project/$_branch/$path';

  String _content(String path) =>
      _edits[_key(path)] ??
      switch (path.split('/').last) {
        'README.md' => context.tr('markdownExample'),
        'package.json' =>
          '{\n  "name": "$_project",\n  "private": true,\n  "scripts": {\n    "dev": "vite",\n    "test": "vitest"\n  }\n}',
        'Login.tsx' =>
          'export function Login() {\n  return (\n    <form className="login-form">\n      <EmailInput />\n      <PasswordInput />\n      <SubmitButton />\n    </form>\n  );\n}',
        'login.css' =>
          '.login-form {\n  display: flex;\n  flex-direction: column;\n  gap: 20px;\n}\n\n.login-button {\n  min-height: 44px;\n}',
        'login.test.ts' =>
          'it("preserves keyboard focus", async () => {\n  await openLogin();\n  await pressTab();\n  expect(email).toBeFocused();\n});',
        'vite.config.ts' =>
          'import { defineConfig } from "vite";\n\nexport default defineConfig({\n  server: { port: 5173 },\n});',
        'favicon.svg' =>
          '<svg xmlns="http://www.w3.org/2000/svg"\n  viewBox="0 0 24 24">\n  <path d="M4 20 12 4 20 20Z"/>\n</svg>',
        _ => 'User-agent: *\nAllow: /',
      };

  Future<void> _search() async {
    var query = '';
    final result = await showAppSheet<String>(
      context,
      context.tr('searchFiles'),
      child: StatefulBuilder(
        builder: (context, update) => Column(
          mainAxisSize: MainAxisSize.min,
          children: [
            TextField(
              autofocus: true,
              decoration: InputDecoration(
                hintText: context.tr('searchFiles'),
                prefixIcon: const Padding(
                  padding: EdgeInsets.all(12),
                  child: AppIcon('search'),
                ),
              ),
              onChanged: (value) => update(() => query = value.toLowerCase()),
            ),
            const SizedBox(height: 12),
            for (final path in _files.where(
              (path) => path.toLowerCase().contains(query),
            ))
              ListTile(
                leading: const AppIcon('file'),
                title: Text(path),
                onTap: () => Navigator.pop(context, path),
              ),
          ],
        ),
      ),
    );
    if (result != null && mounted) _preview(result);
  }

  Future<void> _preview(String path) async {
    final fileKey = _key(path);
    final edit = await showAppSheet<bool>(
      context,
      path.split('/').last,
      child: Builder(
        builder: (context) => Column(
          crossAxisAlignment: CrossAxisAlignment.stretch,
          mainAxisSize: MainAxisSize.min,
          children: [
            Text(path, style: Theme.of(context).textTheme.bodySmall),
            if (_drafts.containsKey(fileKey))
              Text(
                context.tr('unsaved'),
                style: TextStyle(
                  color: Theme.of(context).colorScheme.secondary,
                ),
              ),
            const SizedBox(height: 16),
            Surface(
              child: mediaType(path) == 'text/markdown'
                  ? MarkdownContent(_content(path))
                  : SelectableText(
                      _content(path),
                      style: const TextStyle(
                        fontFamily: 'monospace',
                        fontSize: 14,
                        height: 1.7,
                      ),
                    ),
            ),
            const SizedBox(height: 16),
            FilledButton.icon(
              onPressed: () => Navigator.pop(context, true),
              icon: const AppIcon('code'),
              label: Text(context.tr('edit')),
            ),
          ],
        ),
      ),
    );
    if (edit != true || !mounted) return;
    final value = await showAppSheet<String>(
      context,
      context.tr('edit'),
      child: _FileEditor(
        path: path,
        content: _content(path),
        draft: _drafts[fileKey],
        onChanged: (value) {
          if (value == _content(path)) {
            _drafts.remove(fileKey);
          } else {
            _drafts[fileKey] = value;
          }
        },
        onDiscard: () => _drafts.remove(fileKey),
      ),
    );
    if (value != null && mounted) {
      setState(() {
        _edits[fileKey] = value;
        _drafts.remove(fileKey);
      });
      showResourceNotice(context, 'savePreview');
    }
  }

  @override
  Widget build(BuildContext context) {
    if (AppSession.maybeOf(context) != null) {
      return LiveFilesPage(
        hostId: widget.hostId,
        worktreeId: widget.worktreeId,
      );
    }
    final entries = _directory.isEmpty
        ? ['src/', 'public/', 'package.json', 'README.md', 'vite.config.ts']
        : _files.where((path) => path.startsWith('$_directory/')).toList();
    final colors = Theme.of(context).colorScheme;
    return PageFrame(
      title: context.tr('files'),
      actions: [
        RoundButton(
          icon: 'search',
          tooltip: context.tr('searchFiles'),
          onPressed: _search,
        ),
      ],
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          FileLocation(
            directory: _directory,
            onNavigate: (path) => setState(() => _directory = path),
          ),
          const SizedBox(height: 8),
          Surface(
            padding: const EdgeInsets.symmetric(horizontal: 14),
            child: Column(
              children: [
                for (var index = 0; index < entries.length; index++) ...[
                  if (index > 0) const Divider(),
                  ListTile(
                    contentPadding: EdgeInsets.zero,
                    leading: AppIcon(
                      entries[index].endsWith('/')
                          ? 'folder'
                          : entries[index].endsWith('.md')
                          ? 'file'
                          : 'code',
                    ),
                    title: Text(
                      entries[index]
                          .replaceFirst('$_directory/', '')
                          .replaceAll(RegExp(r'/$'), ''),
                    ),
                    trailing: Text(
                      _edits.containsKey(_key(entries[index])) ||
                              entries[index] == 'package.json'
                          ? 'M'
                          : entries[index].endsWith('/')
                          ? '—'
                          : entries[index].endsWith('.md')
                          ? '3.2 KB'
                          : '1.1 KB',
                      style: TextStyle(
                        color:
                            _edits.containsKey(_key(entries[index])) ||
                                entries[index] == 'package.json'
                            ? colors.secondary
                            : colors.onSurfaceVariant,
                      ),
                    ),
                    onTap: () {
                      final path = entries[index];
                      if (path.endsWith('/')) {
                        setState(
                          () => _directory = path.substring(0, path.length - 1),
                        );
                      } else {
                        _preview(path);
                      }
                    },
                  ),
                ],
              ],
            ),
          ),
        ],
      ),
    );
  }
}

class _FileEditor extends StatefulWidget {
  const _FileEditor({
    required this.path,
    required this.content,
    this.draft,
    required this.onChanged,
    required this.onDiscard,
  });
  final String path;
  final String content;
  final String? draft;
  final ValueChanged<String> onChanged;
  final VoidCallback onDiscard;

  @override
  State<_FileEditor> createState() => _FileEditorState();
}

class _FileEditorState extends State<_FileEditor> {
  late final _controller = TextEditingController(
    text: widget.draft ?? widget.content,
  );

  @override
  void dispose() {
    _controller.dispose();
    super.dispose();
  }

  Future<void> _cancel() async {
    if (_controller.text == widget.content) {
      Navigator.pop(context);
      return;
    }
    final discard = await showAppDialog<bool>(
      context: context,
      builder: (context) => AlertDialog(
        title: Text(context.tr('unsaved')),
        content: Text(context.tr('discardConfirm')),
        actions: [
          TextButton(
            onPressed: () => Navigator.pop(context, false),
            child: Text(context.tr('continueEdit')),
          ),
          FilledButton(
            onPressed: () => Navigator.pop(context, true),
            child: Text(context.tr('discard')),
          ),
        ],
      ),
    );
    if (discard == true && mounted) {
      widget.onDiscard();
      Navigator.pop(context);
    }
  }

  @override
  Widget build(BuildContext context) => Column(
    crossAxisAlignment: CrossAxisAlignment.stretch,
    mainAxisSize: MainAxisSize.min,
    children: [
      Text(widget.path, style: Theme.of(context).textTheme.bodySmall),
      const SizedBox(height: 12),
      TextField(
        controller: _controller,
        onChanged: widget.onChanged,
        minLines: 8,
        maxLines: 16,
        autocorrect: false,
        enableSuggestions: false,
        style: const TextStyle(
          fontFamily: 'monospace',
          fontSize: 14,
          height: 1.6,
        ),
        decoration: InputDecoration(
          labelText: context.tr('edit'),
          alignLabelWithHint: true,
        ),
      ),
      const SizedBox(height: 16),
      Row(
        children: [
          Expanded(
            child: OutlinedButton(
              onPressed: _cancel,
              child: Text(context.tr('cancel')),
            ),
          ),
          const SizedBox(width: 10),
          Expanded(
            child: FilledButton(
              onPressed: () => Navigator.pop(context, _controller.text),
              child: Text(context.tr('save')),
            ),
          ),
        ],
      ),
    ],
  );
}
