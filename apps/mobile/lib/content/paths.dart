/// Resolve a Node file within its captured worktree, never the phone filesystem.
String? resourcePath(
  Uri uri,
  String? root, {
  String directory = '',
  bool allowRoot = false,
  bool lineReference = false,
}) {
  if (uri.path.isEmpty && !allowRoot ||
      uri.hasAuthority && uri.host.isNotEmpty ||
      (uri.scheme.isNotEmpty && uri.scheme != 'file')) {
    return null;
  }
  var source = uri.path;
  if (lineReference) {
    final suffix = RegExp(r':([0-9]+)$').firstMatch(source);
    final fragment = uri.fragment.startsWith('L')
        ? uri.fragment.substring(1)
        : null;
    if (suffix != null && fragment != null) return null;
    final value = suffix?.group(1) ?? fragment;
    if (value != null) {
      final line = int.tryParse(value);
      if (line == null || line < 1 || line > 0xffffffff) return null;
    }
    if (suffix != null) source = source.substring(0, suffix.start);
  }
  String path;
  try {
    path = Uri.decodeComponent(source).replaceAll('\\', '/');
  } on FormatException {
    return null;
  }
  if (path.contains('\u0000')) return null;
  if (RegExp(r'^/[A-Za-z]:/').hasMatch(path)) {
    path = path.substring(1);
  }
  if (path.startsWith('/') || RegExp(r'^[A-Za-z]:/').hasMatch(path)) {
    if (root == null) return null;
    final prefix =
        '${root.replaceAll('\\', '/').replaceFirst(RegExp(r'/+$'), '')}/';
    if (allowRoot && '$path/' == prefix) return '';
    if (!path.startsWith(prefix)) return null;
    path = path.substring(prefix.length);
  } else if (directory.isNotEmpty) {
    path = '$directory/$path';
  }
  final parts = <String>[];
  for (final part in path.split('/')) {
    if (part.isEmpty || part == '.') continue;
    if (part == '..') {
      if (parts.isEmpty) return null;
      parts.removeLast();
    } else {
      parts.add(part);
    }
  }
  return parts.isEmpty && !allowRoot ? null : parts.join('/');
}

/// File name and parent directory of a worktree-relative path.
({String name, String directory}) splitPath(String path) {
  final normalized = path.replaceAll('\\', '/');
  final index = normalized.lastIndexOf('/');
  return index < 0
      ? (name: normalized, directory: '')
      : (
          name: normalized.substring(index + 1),
          directory: normalized.substring(0, index),
        );
}

/// The last [segments] parts of [directory], enough to tell siblings apart.
String shortDirectory(String directory, [int segments = 2]) {
  final parts = directory.split('/').where((part) => part.isNotEmpty).toList();
  return parts
      .skip(parts.length > segments ? parts.length - segments : 0)
      .join('/');
}
