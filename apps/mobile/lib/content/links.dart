import 'package:url_launcher/url_launcher.dart';

final _unsafeUriBytes = RegExp(
  r'[\x00-\x1f\x7f\\]|%(?:0[0-9a-f]|1[0-9a-f]|7f)',
  caseSensitive: false,
);
final _sessionAddress = RegExp(
  r'^sailry://session/[a-z0-9-]+$',
  caseSensitive: false,
);

bool isContentAnchor(Uri uri) =>
    !uri.hasScheme &&
    !uri.hasAuthority &&
    uri.path.isEmpty &&
    !uri.hasQuery &&
    uri.hasFragment;

bool isWebContentLink(Uri uri) =>
    (uri.scheme == 'http' || uri.scheme == 'https') &&
    uri.hasAuthority &&
    uri.host.isNotEmpty &&
    !uri.host.contains('%') &&
    !uri.authority.contains('@') &&
    (!uri.hasPort || uri.port > 0 && uri.port <= 65535);

bool _isMailLink(Uri uri) =>
    uri.scheme == 'mailto' &&
    !uri.hasAuthority &&
    !uri.hasFragment &&
    uri.path.isNotEmpty;

bool isExternalContentLink(Uri uri) =>
    isWebContentLink(uri) || _isMailLink(uri);

Uri? parseContentLink(String? link) {
  if (link == null || _unsafeUriBytes.hasMatch(link)) return null;
  final value = link.trim();
  final uri = Uri.tryParse(value);
  if (uri == null || uri.authority.contains('@')) return null;
  try {
    if (Uri.decodeComponent(uri.path).contains('\\')) return null;
    final valid = switch (uri.scheme) {
      '' => !uri.hasAuthority && (uri.path.isNotEmpty || isContentAnchor(uri)),
      'file' => uri.authority.isEmpty && uri.path.isNotEmpty,
      'http' || 'https' => isWebContentLink(uri),
      'mailto' => _isMailLink(uri),
      'sailry' => _sessionAddress.hasMatch(value),
      'sailry-knowledge' => _isKnowledgeLink(uri),
      'javascript' ||
      'vbscript' ||
      'data' ||
      'blob' ||
      'intent' ||
      'content' ||
      'about' ||
      'chrome' ||
      'chrome-extension' => false,
      _ => true,
    };
    return valid ? uri : null;
  } on FormatException {
    return null;
  }
}

bool _isKnowledgeLink(Uri uri) {
  if (!uri.hasAuthority ||
      uri.host.isEmpty ||
      uri.host.contains('%') ||
      uri.hasPort ||
      uri.hasFragment ||
      uri.pathSegments.length != 1 ||
      uri.pathSegments.single.isEmpty ||
      uri.pathSegments.single.contains('/')) {
    return false;
  }
  final query = uri.queryParametersAll;
  return query['revision']?.singleOrNull?.isNotEmpty == true &&
      query.keys.every((key) => key == 'revision' || key == 'node') &&
      (!query.containsKey('node') ||
          query['node']?.singleOrNull?.isNotEmpty == true);
}

Future<bool> launchExternalContentLink(Uri uri) async =>
    isExternalContentLink(uri) &&
    await launchUrl(uri, mode: LaunchMode.externalApplication);

Uri? parseCitationLink(String value) {
  final uri = parseContentLink(value);
  return uri != null &&
          (isWebContentLink(uri) || uri.scheme == 'sailry-knowledge')
      ? uri
      : null;
}
