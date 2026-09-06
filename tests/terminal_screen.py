"""Replay the cursor/erase operations emitted by Crossterm for PTY assertions."""
import unicodedata
import unittest


def screen_text(data, columns, lines):
    grid = [[" "] * columns for _ in range(lines)]
    row = column = 0
    saved = (0, 0)
    text = data.decode("utf-8", errors="replace")
    index = 0
    while index < len(text):
        char = text[index]
        index += 1
        if char == "\x1b":
            if index >= len(text):
                break
            kind = text[index]
            index += 1
            if kind == "[":
                start = index
                while index < len(text) and not ("@" <= text[index] <= "~"):
                    index += 1
                if index == len(text):
                    break
                args, command = text[start:index], text[index]
                index += 1
                if args.startswith("?"):
                    if args == "?1049" and command == "h":
                        grid = [[" "] * columns for _ in range(lines)]
                        row = column = 0
                    continue
                if any(not (c.isdigit() or c == ";") for c in args):
                    continue  # colors may use colon-separated parameters
                values = [int(value or "0") for value in args.split(";")]
                first = values[0]
                distance = first or 1
                if command in "Hf":
                    row = (first or 1) - 1
                    column = ((values[1] if len(values) > 1 else 1) or 1) - 1
                elif command == "G": column = distance - 1
                elif command == "d": row = distance - 1
                elif command == "A": row -= distance
                elif command == "B": row += distance
                elif command == "C": column += distance
                elif command == "D": column -= distance
                elif command == "E": row, column = row + distance, 0
                elif command == "F": row, column = row - distance, 0
                elif command == "s": saved = (row, column)
                elif command == "u": row, column = saved
                elif command == "J":
                    position = row * columns + column
                    for number in range(lines * columns):
                        if first in (2, 3) or (first == 0 and number >= position) or (first == 1 and number <= position):
                            grid[number // columns][number % columns] = " "
                elif command == "K":
                    for number in range(columns):
                        if first == 2 or (first == 0 and number >= column) or (first == 1 and number <= column):
                            grid[row][number] = " "
                # Styling, cursor visibility and synchronized-update controls
                # do not change text cells.
                row, column = max(0, min(row, lines - 1)), max(0, min(column, columns - 1))
            elif kind == "]":
                while index < len(text) and text[index] != "\x07" and text[index:index + 2] != "\x1b\\":
                    index += 1
                index += 1 if text[index:index + 1] == "\x07" else 2
            elif kind == "7": saved = (row, column)
            elif kind == "8": row, column = saved
            continue
        if char == "\r": column = 0
        elif char == "\n": row += 1
        elif char == "\b": column = max(0, column - 1)
        elif char == "\t": column = min(columns - 1, (column // 8 + 1) * 8)
        elif not unicodedata.category(char).startswith("C"):
            if unicodedata.combining(char):
                if column: grid[row][column - 1] += char
                continue
            width = 2 if unicodedata.east_asian_width(char) in ("W", "F") else 1
            if column + width > columns:
                column = 0
                row += 1
            if row >= lines:
                grid.pop(0)
                grid.append([" "] * columns)
                row = lines - 1
            grid[row][column] = char
            if width == 2: grid[row][column + 1] = ""
            column += width
        if row >= lines:
            grid.pop(0)
            grid.append([" "] * columns)
            row = lines - 1
    return "\n".join("".join(line) for line in grid)


class TerminalScreen(unittest.TestCase):
    def test_cell_differences_form_a_visible_word(self):
        data = b"\x1b[2J\x1b[1;1HWorking\x1b[1;1Hc\x1b[1;3Hmplete"
        self.assertNotIn(b"complete", data)
        self.assertIn("complete", screen_text(data, 80, 24))

    def test_erase_and_partial_control_sequences(self):
        self.assertNotIn("obsolete", screen_text(b"obsolete\r\x1b[2Knew", 80, 24))
        self.assertIn("new", screen_text(b"new\x1b[12;", 80, 24))
        self.assertNotIn("secret", screen_text(b"secret\x1b[?1049hready", 80, 24))

    def test_wide_cells_and_split_utf8(self):
        data = "界a\x1b[1;3Hb".encode()
        self.assertIn("界b", screen_text(data, 80, 24))
        self.assertEqual(screen_text(data[:1] + data[1:], 80, 24), screen_text(data, 80, 24))


if __name__ == "__main__":
    unittest.main()
