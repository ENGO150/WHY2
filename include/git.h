#ifndef WHY2_GIT_H
#define WHY2_GIT_H

#include <git2.h>

int cloneRepository(git_repository *repo, int argc, char **argv); // CLONES GIT REPOSITORY

#endif