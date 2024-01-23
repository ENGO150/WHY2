/*
This is part of WHY2
Copyright (C) 2022 Václav Šmejkal

This program is free software: you can redistribute it and/or modify
it under the terms of the GNU General Public License as published by
the Free Software Foundation, either version 3 of the License, or
(at your option) any later version.

This program is distributed in the hope that it will be useful,
but WITHOUT ANY WARRANTY; without even the implied warranty of
MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
GNU General Public License for more details.

You should have received a copy of the GNU General Public License
along with this program.  If not, see <https://www.gnu.org/licenses/>.
*/

#ifndef WHY2_LOGGER_LOGGER_H
#define WHY2_LOGGER_LOGGER_H

#ifdef __cplusplus
extern "C" {
#endif

#include <why2/logger/flags.h>

why2_log_file why2_init_logger(char *directoryPath); //CREATES LOGGING FILE IN directoryPath
void why2_write_log(int loggerFile, char *logMessage); //WRITES logMessage TO loggerFile

#ifdef __cplusplus
}
#endif

#endif