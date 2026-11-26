package dev.shynewt.shyspace.features.home

/** Development builds can create a workspace on a bucket, to try the app without another device. */
val workspaceCreator: WorkspaceCreator? = { onCreate -> DevelopmentWorkspaceTools(onCreate) }
