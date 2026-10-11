"""Generate owned workbook regressions using the existing document fixture."""
from pathlib import Path
from xml.etree import ElementTree as ET
from zipfile import ZIP_DEFLATED, ZipFile, ZipInfo


def write(path, parts):
    with ZipFile(path, 'w') as archive:
        for name, content in sorted(parts.items()):
            entry = ZipInfo(name, (2025, 1, 1, 0, 0, 0))
            entry.compress_type = ZIP_DEFLATED
            archive.writestr(entry, content)


def generate(root):
    with ZipFile(root / 'book.xlsx') as archive:
        original = {name: archive.read(name) for name in archive.namelist()}

    invalid = dict(original)
    worksheet = invalid['xl/worksheets/sheet1.xml']
    invalid['xl/worksheets/sheet1.xml'] = worksheet.replace(
        b'<row r="1"', b'<row r="1" s="not-an-index"', 1)
    assert worksheet != invalid['xl/worksheets/sheet1.xml']
    write(root / 'invalid-row.xlsx', invalid)

    # Match openpyxl's default drawing namespace and an ElementTree-resaved sheet.
    main = 'http://schemas.openxmlformats.org/spreadsheetml/2006/main'
    relation = 'http://schemas.openxmlformats.org/officeDocument/2006/relationships'
    sheet = ET.Element(f'{{{main}}}worksheet')
    ET.SubElement(sheet, f'{{{main}}}dimension', {'ref': 'A1:B4'})
    data = ET.SubElement(sheet, f'{{{main}}}sheetData')
    for index, values in enumerate([
            ['month', 'revenue'], ['January', 12], ['February', 8], ['Total', 20]], 1):
        row = ET.SubElement(data, f'{{{main}}}row', {'r': str(index)})
        for column, value in zip('AB', values):
            attributes = {'r': f'{column}{index}'}
            if isinstance(value, str):
                attributes['t'] = 'inlineStr'
            cell = ET.SubElement(row, f'{{{main}}}c', attributes)
            if isinstance(value, str):
                inline = ET.SubElement(cell, f'{{{main}}}is')
                ET.SubElement(inline, f'{{{main}}}t').text = value
            else:
                if index == 4:
                    ET.SubElement(cell, f'{{{main}}}f').text = 'SUM(B2:B3)'
                ET.SubElement(cell, f'{{{main}}}v').text = str(value)
    ET.SubElement(sheet, f'{{{main}}}drawing', {f'{{{relation}}}id': 'rId1'})

    chart = dict(original)
    chart['xl/workbook.xml'] = chart['xl/workbook.xml'].replace(
        'name="预算"'.encode(), b'name="Sales"')
    chart['xl/worksheets/sheet1.xml'] = ET.tostring(
        sheet, encoding='utf-8', xml_declaration=True)
    chart['xl/worksheets/_rels/sheet1.xml.rels'] = f'''
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
<Relationship Type="{relation}/drawing" Target="/xl/drawings/drawing1.xml" Id="rId1"/>
</Relationships>'''.strip().encode()
    chart['xl/drawings/drawing1.xml'] = f'''
<wsDr xmlns="http://schemas.openxmlformats.org/drawingml/2006/spreadsheetDrawing">
<twoCellAnchor><from><col>3</col><colOff>0</colOff><row>1</row><rowOff>0</rowOff></from>
<to><col>13</col><colOff>0</colOff><row>18</row><rowOff>0</rowOff></to>
<graphicFrame><nvGraphicFramePr><cNvPr id="1" name="Chart 1"/><cNvGraphicFramePr/></nvGraphicFramePr><xfrm/>
<a:graphic xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main">
<a:graphicData uri="http://schemas.openxmlformats.org/drawingml/2006/chart">
<c:chart xmlns:c="http://schemas.openxmlformats.org/drawingml/2006/chart" xmlns:r="{relation}" r:id="rId1"/>
</a:graphicData></a:graphic></graphicFrame><clientData/></twoCellAnchor></wsDr>'''.strip().encode()
    chart['xl/drawings/_rels/drawing1.xml.rels'] = f'''
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
<Relationship Type="{relation}/chart" Target="/xl/charts/chart1.xml" Id="rId1"/>
</Relationships>'''.strip().encode()
    chart['xl/charts/chart1.xml'] = b'''
<chartSpace xmlns="http://schemas.openxmlformats.org/drawingml/2006/chart"><chart><plotArea>
<barChart><barDir val="col"/><grouping val="clustered"/><ser><idx val="0"/><order val="0"/>
<tx><strRef><f>Sales!$B$1</f></strRef></tx><cat><strRef><f>Sales!$A$2:$A$3</f></strRef></cat>
<val><numRef><f>Sales!$B$2:$B$3</f></numRef></val></ser><gapWidth val="150"/>
<axId val="10"/><axId val="100"/></barChart>
<catAx><axId val="10"/><scaling><orientation val="minMax"/></scaling><axPos val="b"/>
<crossAx val="100"/><crosses val="autoZero"/><auto val="1"/><lblAlgn val="ctr"/><lblOffset val="100"/></catAx>
<valAx><axId val="100"/><scaling><orientation val="minMax"/></scaling><axPos val="l"/>
<crossAx val="10"/><crosses val="autoZero"/><crossBetween val="between"/></valAx>
</plotArea><plotVisOnly val="1"/><dispBlanksAs val="gap"/></chart></chartSpace>'''.strip()
    overrides = b'''
<Override PartName="/xl/drawings/drawing1.xml" ContentType="application/vnd.openxmlformats-officedocument.drawing+xml"/>
<Override PartName="/xl/charts/chart1.xml" ContentType="application/vnd.openxmlformats-officedocument.drawingml.chart+xml"/>
'''
    chart['[Content_Types].xml'] = chart['[Content_Types].xml'].replace(
        b'</Types>', overrides + b'</Types>')
    write(root / 'chart.xlsx', chart)


if __name__ == '__main__':
    generate(Path(__file__).parent)
