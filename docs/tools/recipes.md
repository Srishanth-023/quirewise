# Recipes (Appendix A row 58)

## Purpose

A recipe is a named chain of tools. Each file runs through every step in order, and each step reads the PDF the step before it wrote. Save recipes, rename them, export them as a JSON file for a colleague, import them, and run one on chosen files or on a whole folder. Your originals are never changed and only the result of the last step is kept.

## What can be a step

| Place | Tools |
|---|---|
| First step only | Word to PDF, PowerPoint to PDF, Excel to PDF (they turn another file type into a PDF) |
| Any step | Rotate, Insert pages, Compress, Repair, Remove annotations, Flatten, Edit metadata, Unlock, Sanitize, Linearize, Page numbers, Header and footer, Bates numbering, Watermark, Crop, Resize, Flip, N-up, Grayscale, Bookmarks, Attachments, Page labels, Extract pages, Remove pages, Translate PDF, PDF to PDF/A, Redact, OCR, Deskew |
| Last step only | The splits, Smart Split, Rename by content, Protect, Digital signature, AI Summarizer, Validate PDF/A, PDF/UA check, Verify signatures, and every conversion from PDF (Word, PowerPoint, Excel, Markdown, HTML, images, text, TIFF) |
| Not in recipes | Merge, Alternate and mix, Organize, Compare, Edit PDF, Annotate, Fill and sign, Sign PDF, Create forms, Scan to PDF, HTML to PDF, Images to PDF, TIFF to PDF (they need the editor, several files or a person) |

The core enforces the same rules when a recipe is saved, imported or run (`domain::recipe::ChainRole`). A step that writes several files, or a report, ends the chain because the next step would have nothing single to continue with.

## Options

A recipe is `{ "id", "name", "steps": [ { "options": { "tool": "rotate", ... } } ] }`. The options of a step are exactly the options of that tool (see its page), with the tool name as the `tool` tag. At most 20 steps and 80 characters of name.

## Export and import

Export writes `{ "format": "quirewise-recipe", "version": 1, "recipe": { ... } }` without the id. Import checks the format marker, refuses newer versions, runs the same chain checks, reads at most 1 MiB and always adds a new entry. Passwords are not options of any recipe-capable step except Protect and Digital signature; a recipe file with those options holds what the person typed, so share such files with the same care as the passwords.

## Running

- **Files.** Add files in the app, or give them on the command line.
- **Folder.** The PDFs of a folder, plus Word, PowerPoint and Excel files when the first step converts them, optionally with sub-folders (down to 8 levels, at most 2000 files). Links, junctions, hidden files and Office lock files (`~$name`) are skipped, so a folder run never leaves the folder through a link.
- **Output.** Next to each source, or in a folder you choose. The naming pattern applies to the last step. Intermediate files live in the session scratch folder under the plain `{filename}` pattern and are removed after each step, also when a step fails.
- **Status.** One line per file, with the step that failed named in the message ("Step 2 (compress): ..."). Retry failed files runs only those again. Cancel stops after the current step.
- **Progress.** One bar per file that covers all its steps.

```
qw recipe tidy.json --folder D:\Scans --recursive --out D:\Done
qw recipe tidy.json a.pdf b.pdf --password-stdin
```

## Errors

`QW-OPTION-INVALID` (empty name, no steps, a step in a place it cannot be, a step that wrote several files, nothing to process, not a recipe file, too large), and every error of the tool that failed, with the step named. `QW-JOB-CANCELLED` when cancelled.

## License impact

None.
