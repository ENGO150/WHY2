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

#include <why2/logger/flags.h>

logFile initLogger(char *directoryPath); //CREATES LOGGING FILE IN directoryPath
void writeLog(int loggerFile, char *logMessage); //WRITES logMessage TO loggerFile

#endif