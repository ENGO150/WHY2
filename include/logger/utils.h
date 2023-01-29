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

#ifndef WHY2_LOGGER_UTILS_C
#define WHY2_LOGGER_UTILS_C

#include <why2/logger/flags.h>

void why2_deallocate_logger(why2_log_file logger); //USE THIS IF YOU WANT TO DEALLOCATE FILE POINTER RETURNED BY logger'S why2_init_logger
void why2_deallocate_decrypted_output(why2_decrypted_output output); //DEALLOCATION OF POINTER-TO-POINTER, WHY TF ARE YOU READING THIS
why2_decrypted_output why2_decrypt_logger(why2_log_file logger); //PASS logger AND FLAGS, AND PROGRAM WILL DECRYPT YOUR LOG... WHAT DID YOU EXPECT?

#endif