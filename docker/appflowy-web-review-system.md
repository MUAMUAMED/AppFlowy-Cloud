# AppFlowy Web review system

The custom web image is based on AppFlowy Web `0.10.0`, the version compatible with this public AppFlowy Cloud backend.

To reproduce the image:

1. Clone `https://github.com/AppFlowy-IO/AppFlowy-Web.git` at tag `0.10.0`.
2. Apply `docker/appflowy-web-review-system.patch` from this repository.
3. Build with `docker build -f docker/Dockerfile -t appflowyinc/appflowy_web:review-system .`.

The patch adds the Review route and navigation, global flashcard creation, taxonomy management, daily queue UI, XP/streak display, and the three review formats. It also adds the `Espaço de estudo` workspace template selector and a persistent `Tipo de estudo` property to every page. Pages classified as `Matéria` automatically synchronize their child tree into the Review taxonomy (subject → topic → content). Legacy titles marked with `#materia` are migrated to the property and cleaned automatically.

The AI-assisted bulk importer is available from Review and through the `/flashcards` editor command. It validates the versioned `studyflashcards.v1` JSON format and imports classic, true/false, and multiple-choice cards atomically. Cards can be linked to their source page, allowing an incorrect answer to open that content in a modal without ending the review session.

The sidebar also includes an Obsidian-inspired Graph View. It maps the workspace page hierarchy, distinguishes spaces, subjects, topics, and content by color, and supports searching, filtering, zooming, panning, dragging nodes, and opening any page in the existing modal.
