/// File sizes use decimal units (1 KB = 1,000 bytes).
String fileSize(int? bytes) {
  if (bytes == null || bytes < 0) return '—';
  const units = ['B', 'KB', 'MB', 'GB', 'TB', 'PB', 'EB'];
  var value = bytes.toDouble();
  var unit = 0;
  while (value >= 1000 && unit < units.length - 1) {
    value /= 1000;
    unit++;
  }
  // Promote values that round to the next unit rather than showing 1000 KB.
  if (unit > 0 && value >= 999.95 && unit < units.length - 1) {
    value /= 1000;
    unit++;
  }
  final amount = unit == 0
      ? bytes.toString()
      : value.toStringAsFixed(1).replaceFirst(RegExp(r'\.0$'), '');
  return '$amount ${units[unit]}';
}
