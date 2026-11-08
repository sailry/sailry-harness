# ModelDial public radar data

Source: [ModelDial Radar](https://modeldial.com/radar), downloaded from
`https://modeldial.com/data/overview.json`.

The fixture `crates/node-runtime/tests/fixtures/model-scores.json` is a modified,
reduced copy of batch `overall-f26004137653ddaeb3be2933`, published
`2026-11-07T07:48:26.226Z`. Entries and unused fields were removed; retained
measurements are unchanged. Runtime data is normalized, matched to configured
model identifiers and summarized for display. Repeated measurements for the
same model and effort use the first candidate in ascending identifier order;
candidate identifiers are not interpreted as timestamps.

The public dataset is licensed under
[Creative Commons Attribution 4.0 International](https://creativecommons.org/licenses/by/4.0/),
as stated in [ModelDial's data license](https://modeldial.com/data-license).
ModelDial does not endorse Sailry or its presentation of the data. No ModelDial
logo, page design, evaluation prompts or other unlicensed assets are included.
