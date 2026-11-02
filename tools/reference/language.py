"""Source `ls` word table and the installed language files of build 11140890."""
from .caps import default_words, managed_code, parse_word_names

LANGUAGES = 'TheLongDrive_Data/Languages/'


def parse_language(text: str, count: int) -> dict:
    """Source `ls.ScanLanguages` and `LoadSelectedLanguage`: the first line is the name, then one word per line.
    A missing or empty line is None. The game shows the English word for it."""
    lines = text.split('\n')
    lines = [line[:-1] if line.endswith('\r') else line for line in lines]
    words = [(lines[i + 1] or None) if i + 1 < len(lines) else None for i in range(count)]
    return {'name': lines[0], 'words': words}


def recover_language(source) -> dict:
    """Word names in `ls.w` order, the English words of `ls.FillDefWW` and each language file in directory order.
    `ScanLanguages` skips files whose path contains `sample`."""
    names = parse_word_names(managed_code(source))
    english = default_words(source)
    if len(names) != len(english):
        raise ValueError('Language enum and default table differ')
    library = source.library
    paths = sorted((p for p in library.source_files if p.startswith(LANGUAGES) and p.endswith('.txt')
                    and 'sample' not in p), key=str.lower)
    languages = [parse_language(library.source_range(p, 0, library.source_files[p].bytes).decode('utf-8-sig'), len(names))
                 for p in paths]
    if not languages:
        raise ValueError('Missing source language files')
    return {'names': names, 'english': english, 'languages': languages}
