#!/usr/bin/env -S uv run --script
# /// script
# requires-python = ">=3.10"
# dependencies = [
#   "markdown-it-py==4.0.0",
#   "reportlab==4.4.3",
# ]
# ///
"""Render the Sol Markdown cheatsheet as a compact, card-based PDF."""

from __future__ import annotations

import argparse
import html
import re
from dataclasses import dataclass
from pathlib import Path

import reportlab
from markdown_it import MarkdownIt
from markdown_it.token import Token
from reportlab.lib import colors
from reportlab.lib.pagesizes import landscape, letter
from reportlab.lib.styles import ParagraphStyle
from reportlab.lib.units import inch
from reportlab.pdfbase import pdfmetrics
from reportlab.pdfbase.ttfonts import TTFont
from reportlab.platypus import (
    BaseDocTemplate,
    Flowable,
    Frame,
    KeepTogether,
    PageTemplate,
    Paragraph,
    Preformatted,
    Spacer,
    Table,
    TableStyle,
)

PAGE_WIDTH, PAGE_HEIGHT = landscape(letter)
MARGIN = 0.34 * inch
GAP = 0.12 * inch
COLUMN_WIDTH = (PAGE_WIDTH - 2 * MARGIN - GAP) / 2
CONTENT_WIDTH = COLUMN_WIDTH - 12
FIRST_HEADER = 0.92 * inch

INK = colors.HexColor("#242424")
MUTED = colors.HexColor("#747474")
PAGE_BG = colors.HexColor("#F4F4F4")
WHITE = colors.white
BLACK = colors.HexColor("#000000")
MAROON = colors.HexColor("#8C1D40")
GOLD = colors.HexColor("#FFC627")
GOLD_PALE = colors.HexColor("#FFF4CF")
RULE = colors.HexColor("#D5D5D5")
CODE_BG = colors.HexColor("#161616")
CODE_FG = colors.HexColor("#F7F7F7")

CARD_TONES = {
    "maroon": (MAROON, WHITE),
    "gold": (GOLD, BLACK),
    "black": (BLACK, WHITE),
}


@dataclass
class Block:
    kind: str
    value: object


@dataclass
class Section:
    title: str
    blocks: list[Block]


def register_fonts() -> None:
    """Use host fonts when available, with PDF core-font fallbacks."""
    bundled_fonts = Path(reportlab.__file__).parent / "fonts"
    candidates = {
        "SolSans": [
            "/usr/share/fonts/dejavu-sans-fonts/DejaVuSans.ttf",
            "/usr/share/fonts/dejavu/DejaVuSans.ttf",
            "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
            str(bundled_fonts / "Vera.ttf"),
        ],
        "SolSansBold": [
            "/usr/share/fonts/dejavu-sans-fonts/DejaVuSans-Bold.ttf",
            "/usr/share/fonts/dejavu/DejaVuSans-Bold.ttf",
            "/usr/share/fonts/truetype/dejavu/DejaVuSans-Bold.ttf",
            str(bundled_fonts / "VeraBd.ttf"),
        ],
        "SolMono": [
            "/usr/share/fonts/dejavu-sans-mono-fonts/DejaVuSansMono.ttf",
            "/usr/share/fonts/dejavu/DejaVuSansMono.ttf",
            "/usr/share/fonts/truetype/dejavu/DejaVuSansMono.ttf",
            str(bundled_fonts / "VeraMono.ttf"),
        ],
    }
    for name, paths in candidates.items():
        for path in paths:
            if Path(path).exists():
                pdfmetrics.registerFont(TTFont(name, path))
                break

    missing = {"SolSans", "SolSansBold", "SolMono"} - set(
        pdfmetrics.getRegisteredFontNames()
    )
    if missing:
        raise RuntimeError(
            f"could not register PDF fonts: {', '.join(sorted(missing))}"
        )


def inline_markup(token: Token) -> str:
    parts: list[str] = []
    for child in token.children or []:
        if child.type == "text":
            parts.append(html.escape(child.content))
        elif child.type in {"softbreak", "hardbreak"}:
            parts.append(" ")
        elif child.type == "code_inline":
            value = html.escape(child.content)
            parts.append(f'<font name="SolMono" color="#8C1D40">{value}</font>')
        elif child.type == "strong_open":
            parts.append("<b>")
        elif child.type == "strong_close":
            parts.append("</b>")
        elif child.type == "em_open":
            parts.append("<i>")
        elif child.type == "em_close":
            parts.append("</i>")
        elif child.type == "link_open":
            href = html.escape(child.attrGet("href") or "", quote=True)
            parts.append(f'<link href="{href}" color="#8C1D40">')
        elif child.type == "link_close":
            parts.append("</link>")
        elif child.type == "html_inline":
            parts.append(html.escape(child.content))
        else:
            parts.append(html.escape(child.content))
    return "".join(parts)


def plain_text(markup: str) -> str:
    return html.unescape(re.sub(r"<[^>]+>", "", markup))


def parse_table(tokens: list[Token], start: int) -> tuple[Block, int]:
    rows: list[list[str]] = []
    row: list[str] | None = None
    i = start + 1
    while i < len(tokens) and tokens[i].type != "table_close":
        token = tokens[i]
        if token.type == "tr_open":
            row = []
        elif token.type == "inline" and row is not None:
            row.append(inline_markup(token))
        elif token.type == "tr_close" and row is not None:
            rows.append(row)
            row = None
        i += 1
    return Block("table", rows), i + 1


def parse_markdown(source: str) -> tuple[str, list[Section]]:
    tokens = MarkdownIt("commonmark").enable("table").parse(source)
    intro = ""
    sections: list[Section] = []
    current: Section | None = None
    quote_depth = 0
    i = 0
    while i < len(tokens):
        token = tokens[i]
        if token.type == "heading_open" and token.tag == "h2":
            title = plain_text(inline_markup(tokens[i + 1]))
            current = Section(title, [])
            sections.append(current)
            i += 3
            continue
        if token.type == "blockquote_open":
            quote_depth += 1
            i += 1
            continue
        if token.type == "blockquote_close":
            quote_depth = max(0, quote_depth - 1)
            i += 1
            continue
        if token.type == "paragraph_open" and i + 1 < len(tokens):
            markup = inline_markup(tokens[i + 1])
            if current is None:
                intro = plain_text(markup)
            else:
                current.blocks.append(
                    Block("quote" if quote_depth else "paragraph", markup)
                )
            i += 3
            continue
        if token.type == "fence" and current is not None:
            current.blocks.append(Block("code", token.content.rstrip()))
            i += 1
            continue
        if token.type == "table_open" and current is not None:
            block, i = parse_table(tokens, i)
            current.blocks.append(block)
            continue
        i += 1
    return intro, sections


BODY_STYLE = ParagraphStyle(
    "Body",
    fontName="SolSans",
    fontSize=6.55,
    leading=8.0,
    textColor=INK,
    spaceAfter=3,
)
SMALL_STYLE = ParagraphStyle(
    "Small",
    parent=BODY_STYLE,
    fontSize=6.05,
    leading=7.25,
)
CARD_TITLE_LIGHT_STYLE = ParagraphStyle(
    "CardTitle",
    fontName="SolSansBold",
    fontSize=8.2,
    leading=9.2,
    textColor=WHITE,
    spaceAfter=0,
)
CARD_TITLE_DARK_STYLE = ParagraphStyle(
    "CardTitleDark",
    parent=CARD_TITLE_LIGHT_STYLE,
    textColor=BLACK,
)
TABLE_CELL_STYLE = ParagraphStyle(
    "TableCell",
    parent=SMALL_STYLE,
    fontSize=6.05,
    leading=7.1,
    spaceAfter=0,
)
TABLE_HEAD_STYLE = ParagraphStyle(
    "TableHead",
    parent=TABLE_CELL_STYLE,
    fontName="SolSansBold",
    textColor=WHITE,
)
CODE_STYLE = ParagraphStyle(
    "Code",
    fontName="SolMono",
    fontSize=5.4,
    leading=6.6,
    textColor=CODE_FG,
    spaceAfter=0,
)
QUOTE_STYLE = ParagraphStyle(
    "Quote",
    parent=BODY_STYLE,
    fontSize=6.2,
    leading=7.6,
    textColor=colors.HexColor("#4B4638"),
    spaceAfter=0,
)


def table_widths(rows: list[list[str]]) -> list[float]:
    columns = len(rows[0]) if rows else 1
    first = plain_text(rows[0][0]).lower() if rows and rows[0] else ""
    if columns == 2:
        ratio = 0.38 if first in {"command", "reason", "situation"} else 0.34
        return [CONTENT_WIDTH * ratio, CONTENT_WIDTH * (1 - ratio)]
    if columns == 3:
        if first == "partition":
            ratios = [0.22, 0.18, 0.60]
        elif first in {"you want", "path"}:
            ratios = [0.28, 0.27, 0.45]
        else:
            ratios = [0.25, 0.22, 0.53]
        return [CONTENT_WIDTH * value for value in ratios]
    return [CONTENT_WIDTH / columns] * columns


def data_table(rows: list[list[str]]) -> Table:
    normalized = [
        [
            Paragraph(value, TABLE_HEAD_STYLE if row_index == 0 else TABLE_CELL_STYLE)
            for value in row
        ]
        for row_index, row in enumerate(rows)
    ]
    table = Table(normalized, colWidths=table_widths(rows), repeatRows=1, hAlign="LEFT")
    style = [
        ("BACKGROUND", (0, 0), (-1, 0), MAROON),
        ("TEXTCOLOR", (0, 0), (-1, 0), WHITE),
        ("VALIGN", (0, 0), (-1, -1), "TOP"),
        ("LEFTPADDING", (0, 0), (-1, -1), 3.5),
        ("RIGHTPADDING", (0, 0), (-1, -1), 3.5),
        ("TOPPADDING", (0, 0), (-1, -1), 2.5),
        ("BOTTOMPADDING", (0, 0), (-1, -1), 2.5),
        ("GRID", (0, 0), (-1, -1), 0.35, RULE),
        ("ROWBACKGROUNDS", (0, 1), (-1, -1), [WHITE, colors.HexColor("#F4F4F4")]),
    ]
    table.setStyle(TableStyle(style))
    return table


def quote_box(markup: str) -> Table:
    table = Table([[Paragraph(markup, QUOTE_STYLE)]], colWidths=[CONTENT_WIDTH])
    table.setStyle(
        TableStyle(
            [
                ("BACKGROUND", (0, 0), (-1, -1), GOLD_PALE),
                ("BOX", (0, 0), (-1, -1), 0.5, colors.HexColor("#E9C86F")),
                ("LINEBEFORE", (0, 0), (0, -1), 2.2, GOLD),
                ("LEFTPADDING", (0, 0), (-1, -1), 6),
                ("RIGHTPADDING", (0, 0), (-1, -1), 6),
                ("TOPPADDING", (0, 0), (-1, -1), 5),
                ("BOTTOMPADDING", (0, 0), (-1, -1), 5),
            ]
        )
    )
    return table


def code_box(value: str) -> Table:
    preformatted = Preformatted(value, CODE_STYLE, maxLineLength=84)
    table = Table([[preformatted]], colWidths=[CONTENT_WIDTH])
    table.setStyle(
        TableStyle(
            [
                ("BACKGROUND", (0, 0), (-1, -1), CODE_BG),
                ("LINEBEFORE", (0, 0), (0, -1), 2.2, GOLD),
                ("LEFTPADDING", (0, 0), (-1, -1), 6),
                ("RIGHTPADDING", (0, 0), (-1, -1), 6),
                ("TOPPADDING", (0, 0), (-1, -1), 5),
                ("BOTTOMPADDING", (0, 0), (-1, -1), 5),
                ("ROUNDEDCORNERS", [4, 4, 4, 4]),
            ]
        )
    )
    return table


def block_flowables(blocks: list[Block], compact: bool = False) -> list[Flowable]:
    flowables: list[Flowable] = []
    for block in blocks:
        if block.kind == "paragraph":
            style = SMALL_STYLE if compact else BODY_STYLE
            flowables.append(Paragraph(str(block.value), style))
        elif block.kind == "quote":
            flowables.extend([quote_box(str(block.value)), Spacer(1, 3)])
        elif block.kind == "code":
            flowables.extend([code_box(str(block.value)), Spacer(1, 3)])
        elif block.kind == "table":
            flowables.extend([data_table(block.value), Spacer(1, 2)])
    return flowables


def card(
    category: str,
    title: str,
    blocks: list[Block],
    tone: str,
    compact: bool = False,
) -> Flowable:
    accent, title_color = CARD_TONES[tone]
    title_style = (
        CARD_TITLE_DARK_STYLE if title_color == BLACK else CARD_TITLE_LIGHT_STYLE
    )
    heading = Paragraph(
        f"{html.escape(category.upper())}  /  {html.escape(title.upper())}",
        title_style,
    )
    body = block_flowables(blocks, compact=compact)
    body_padding = 3.5 if compact else 5
    table = Table([[heading], [body]], colWidths=[COLUMN_WIDTH], hAlign="LEFT")
    table.setStyle(
        TableStyle(
            [
                ("BACKGROUND", (0, 0), (-1, 0), accent),
                ("BACKGROUND", (0, 1), (-1, -1), WHITE),
                ("BOX", (0, 0), (-1, -1), 0.6, accent),
                ("LINEBELOW", (0, 0), (-1, 0), 2.2, GOLD),
                ("LEFTPADDING", (0, 0), (-1, 0), 7),
                ("RIGHTPADDING", (0, 0), (-1, 0), 7),
                ("TOPPADDING", (0, 0), (-1, 0), 5),
                ("BOTTOMPADDING", (0, 0), (-1, 0), 5),
                ("LEFTPADDING", (0, 1), (-1, -1), 6),
                ("RIGHTPADDING", (0, 1), (-1, -1), 6),
                ("TOPPADDING", (0, 1), (-1, -1), body_padding),
                ("BOTTOMPADDING", (0, 1), (-1, -1), body_padding),
                ("ROUNDEDCORNERS", [6, 6, 6, 6]),
            ]
        )
    )
    return KeepTogether([table, Spacer(1, 6)])


def make_story(sections: list[Section]) -> list[Flowable]:
    story: list[Flowable] = []
    # Keep the second page balanced: the compact rules card closes the left
    # column, while diagnosis starts the right-hand troubleshooting column.
    remember = next(section for section in sections if section.title == "Five rules to remember")
    sections = [section for section in sections if section is not remember]
    diagnose_index = next(
        index
        for index, section in enumerate(sections)
        if section.title == "Job stuck PENDING? Diagnose before rerouting"
    )
    sections.insert(diagnose_index, remember)
    section_design = {
        "Know your access first": ("Access", "maroon"),
        "Everyday solx workflow": ("Workflow", "maroon"),
        "Complete solx command map": ("Commands", "maroon"),
        "Output, targeting, and safety rules": ("Safety", "gold"),
        "solx keep - bounded scratch renewal": ("Scratch", "maroon"),
        "Slurm basics": ("Batch", "maroon"),
        "solx and raw Slurm equivalents": ("Translate", "black"),
        "Job stuck PENDING? Diagnose before rerouting": ("Diagnose", "gold"),
        "Human wrappers and agent-parseable commands": ("Automation", "maroon"),
        "Reach a compute-node service from your laptop": ("Connect", "black"),
        "Storage and heavy I/O": ("Storage", "maroon"),
        "Five rules to remember": ("Remember", "gold"),
    }
    for section in sections:
        if section.title == "Pick a partition by wall-time, not by GPU use":
            paragraphs = [
                block for block in section.blocks if block.kind == "paragraph"
            ]
            tables = [block for block in section.blocks if block.kind == "table"]
            quotes = [block for block in section.blocks if block.kind == "quote"]
            if tables:
                story.append(
                    card("Routing", "Partitions", paragraphs + tables[:1], "maroon")
                )
            if len(tables) > 1:
                story.append(card("Access", "QOS overlays", tables[1:2], "black"))
            if quotes:
                story.append(card("Decision", "Routing in one glance", quotes, "gold"))
            continue
        plain_title = plain_text(section.title)
        category, tone = section_design[plain_title]
        compact = plain_title == "solx keep - bounded scratch renewal"
        story.append(card(category, section.title, section.blocks, tone, compact))
    return story


def draw_footer(canvas, page_number: int) -> None:
    canvas.setFillColor(MUTED)
    canvas.setFont("SolSans", 5.6)
    canvas.drawString(MARGIN, 0.18 * inch, "github.com/Shu-Wan/solx")
    canvas.setFont("SolMono", 5.5)
    canvas.drawRightString(PAGE_WIDTH - MARGIN, 0.18 * inch, f"{page_number:02d}")


def draw_first_page(canvas, doc, intro: str, version: str) -> None:
    canvas.saveState()
    canvas.setTitle("Sol Quick Reference")
    canvas.setAuthor("Shu-Wan / solx")
    canvas.setFillColor(PAGE_BG)
    canvas.rect(0, 0, PAGE_WIDTH, PAGE_HEIGHT, stroke=0, fill=1)

    x = MARGIN
    y = PAGE_HEIGHT - MARGIN - 0.67 * inch
    width = PAGE_WIDTH - 2 * MARGIN
    height = 0.64 * inch
    canvas.setFillColor(MAROON)
    canvas.roundRect(x, y, width, height, 10, stroke=0, fill=1)
    canvas.setFillColor(GOLD)
    canvas.rect(x, y, 5, height, stroke=0, fill=1)
    canvas.setFillColor(WHITE)
    canvas.setFont("SolSansBold", 24)
    canvas.drawString(x + 16, y + 22, "Sol Quick Reference")
    canvas.setFont("SolSans", 7.2)
    canvas.setFillColor(colors.HexColor("#F1DDE4"))
    canvas.drawString(x + 17, y + 10, intro)

    pill_width = 0.68 * inch
    canvas.setFillColor(GOLD)
    canvas.roundRect(
        PAGE_WIDTH - MARGIN - pill_width - 12,
        y + 21,
        pill_width,
        16,
        8,
        stroke=0,
        fill=1,
    )
    canvas.setFillColor(BLACK)
    canvas.setFont("SolMono", 6.7)
    canvas.drawCentredString(
        PAGE_WIDTH - MARGIN - pill_width / 2 - 12, y + 26.2, f"v{version}"
    )

    canvas.setFillColor(GOLD)
    canvas.setFont("SolMono", 15)
    canvas.drawRightString(PAGE_WIDTH - MARGIN - 14, y + 7, ">_")

    draw_footer(canvas, doc.page)
    canvas.restoreState()


def draw_later_page(canvas, doc, version: str) -> None:
    canvas.saveState()
    canvas.setFillColor(PAGE_BG)
    canvas.rect(0, 0, PAGE_WIDTH, PAGE_HEIGHT, stroke=0, fill=1)
    canvas.setFillColor(MAROON)
    canvas.setFont("SolSansBold", 6.5)
    canvas.drawString(MARGIN, PAGE_HEIGHT - 0.24 * inch, "ASU SOL  /  QUICK REFERENCE")
    canvas.setFillColor(MUTED)
    canvas.setFont("SolMono", 6)
    canvas.drawRightString(
        PAGE_WIDTH - MARGIN, PAGE_HEIGHT - 0.24 * inch, f"solx v{version}"
    )
    canvas.setStrokeColor(GOLD)
    canvas.setLineWidth(1.2)
    canvas.line(
        MARGIN, PAGE_HEIGHT - 0.3 * inch, PAGE_WIDTH - MARGIN, PAGE_HEIGHT - 0.3 * inch
    )
    draw_footer(canvas, doc.page)
    canvas.restoreState()


def frames(first: bool) -> list[Frame]:
    y = MARGIN
    top_reserved = FIRST_HEADER if first else 0.16 * inch
    height = PAGE_HEIGHT - 2 * MARGIN - top_reserved
    return [
        Frame(
            MARGIN + index * (COLUMN_WIDTH + GAP),
            y,
            COLUMN_WIDTH,
            height,
            leftPadding=0,
            rightPadding=0,
            topPadding=0,
            bottomPadding=0,
            id=f"{'first' if first else 'later'}-{index}",
        )
        for index in range(2)
    ]


def render(source: Path, output: Path, version: str) -> None:
    register_fonts()
    intro, sections = parse_markdown(source.read_text())
    output.parent.mkdir(parents=True, exist_ok=True)
    doc = BaseDocTemplate(
        str(output),
        pagesize=landscape(letter),
        leftMargin=MARGIN,
        rightMargin=MARGIN,
        topMargin=MARGIN,
        bottomMargin=MARGIN,
        title="Sol Quick Reference",
        author="Shu-Wan / solx",
    )
    first = PageTemplate(
        id="First",
        frames=frames(True),
        onPage=lambda canvas, active_doc: draw_first_page(
            canvas, active_doc, intro, version
        ),
        autoNextPageTemplate="Later",
    )
    later = PageTemplate(
        id="Later",
        frames=frames(False),
        onPage=lambda canvas, active_doc: draw_later_page(canvas, active_doc, version),
    )
    doc.addPageTemplates([first, later])
    doc.build(make_story(sections))


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("source", type=Path)
    parser.add_argument("output", type=Path)
    parser.add_argument("--version", required=True)
    args = parser.parse_args()
    render(args.source, args.output, args.version)


if __name__ == "__main__":
    main()
