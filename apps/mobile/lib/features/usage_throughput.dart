({String rate, int responses})? readThroughput(Object? value) {
  if (value case {
    'elapsed_us': final int elapsed,
    'output_tokens': final int output,
    'responses': final int responses,
  } when elapsed > 0 && output >= 0 && responses > 0) {
    return (
      rate: (output / (elapsed / Duration.microsecondsPerSecond))
          .toStringAsFixed(1),
      responses: responses,
    );
  }
  return null;
}
