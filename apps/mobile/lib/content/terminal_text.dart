final _escape = RegExp(
  // OSC strings, CSI sequences, then any other two-byte escape.
  r'\x1B\][^\x07\x1B]*(?:\x07|\x1B\\)|\x1B\[[0-?]*[ -/]*[@-~]|\x1B[@-Z\\-_]',
);
final _control = RegExp(r'[\x00-\x08\x0B\x0C\x0E-\x1F\x7F]');

/// Terminal output as a person would read it: escape sequences removed and
/// carriage-return redraws (spinners, progress bars) reduced to their last frame.
String plainTerminalText(String value) {
  if (!value.contains('\x1B') &&
      !value.contains('\r') &&
      !_control.hasMatch(value)) {
    return value;
  }
  final lines = value
      .replaceAll(_escape, '')
      .replaceAll('\r\n', '\n')
      .split('\n');
  return [
    for (final line in lines)
      line
          .split('\r')
          .lastWhere((frame) => frame.isNotEmpty, orElse: () => '')
          .replaceAll(_control, ''),
  ].join('\n');
}
