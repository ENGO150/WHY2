/*
This is part of WHY2
Copyright (C) 2022-2025 Václav Šmejkal

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

//CONSTS
pub enum ExitCode //exit codes you fucking idiot
{
    Success = 0, //EXIT CODE FOR WHY2_SUCCESSFUL RUN
    InvalidKey = 1, //EXIT CODE FOR INVALID KEY
    InvalidText = 4, //EXIT CODE FOR INVALID TEXT
    DownloadFailed = 2, //EXIT CODE FOR versions.json DOWNLOAD FAIL
}

pub struct WHY2Data
{
    output: String, //ENCRYPTED/DECRYPTED TEST
    key: String, //KEY USED FOR ENCRYPTION
    exit_code: ExitCode, //EXIT CODE
}
