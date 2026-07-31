script_name('Custom Skin Loader UI')
script_author('Akionka')
script_version('0.1.0')
script_properties('work-in-pause')

local ffi = require 'ffi'
local imgui = require 'mimgui'
local new = imgui.new

local CONFIG_PATH = getGameDirectory() .. [[\custom_skin_loader.json]]
local TEMP_CONFIG_PATH = CONFIG_PATH .. '.tmp'
local MOVEFILE_REPLACE_EXISTING = 0x1
local MOVEFILE_WRITE_THROUGH = 0x8

ffi.cdef [[
  int __stdcall MoveFileExA(const char* existingFileName, const char* newFileName, unsigned long flags);
]]

local kernel32 = ffi.load('kernel32')
local INPUT_TEXT_FLAGS_NONE = imgui.InputTextFlags.None

local function input_text(label, buffer)
  return imgui.InputText(
    label,
    buffer,
    ffi.sizeof(buffer),
    INPUT_TEXT_FLAGS_NONE,
    nil,
    nil
  )
end

local function input_text_with_hint(label, hint, buffer)
  return imgui.InputTextWithHint(
    label,
    hint,
    buffer,
    ffi.sizeof(buffer),
    INPUT_TEXT_FLAGS_NONE,
    nil,
    nil
  )
end

local function input_int(label, value)
  return imgui.InputInt(label, value, 1, 100, INPUT_TEXT_FLAGS_NONE)
end

local state = {
  config = { skins = {}, players = {} },
  window_open = new.bool(false),
  dirty = false,
  status = 'Use /skins to open this editor.',
  status_is_error = false,
  selected_skin = nil,
  selected_player = nil,
  profile_id = new.char[64](),
  txd_path = new.char[260](),
  dff_path = new.char[260](),
  donor_model_id = new.int(7),
  profile_enabled = new.bool(true),
  player_name = new.char[64](),
  player_skin_id = new.char[64](),
  player_enabled = new.bool(true),
  profile_search = new.char[64](),
}

local function buffer_value(buffer)
  return ffi.string(buffer)
end

local function set_buffer(buffer, value)
  imgui.StrCopy(buffer, value or '')
end

local function trim(value)
  return value:match('^%s*(.-)%s*$')
end

local function sorted_keys(map)
  local keys = {}
  for key in pairs(map) do
    table.insert(keys, key)
  end
  table.sort(keys)
  return keys
end

local function set_status(message, is_error)
  state.status = message
  state.status_is_error = is_error or false
end

local function ensure_schema(config)
  if type(config) ~= 'table' then config = {} end
  if type(config.skins) ~= 'table' then config.skins = {} end
  if type(config.players) ~= 'table' then config.players = {} end

  for _, skin in pairs(config.skins) do
    if type(skin) == 'table' and skin.enabled == nil then
      skin.enabled = true
    end
  end
  for player_name, assignment in pairs(config.players) do
    if type(assignment) == 'string' then
      config.players[player_name] = { skin_id = assignment, enabled = true }
    elseif type(assignment) == 'table' and assignment.enabled == nil then
      assignment.enabled = true
    end
  end

  return config
end

local function validate_config()
  for skin_id, skin in pairs(state.config.skins) do
    if skin_id == '' then
      return false, 'A profile has an empty ID.'
    end
    if type(skin) ~= 'table' or type(skin.enabled) ~= 'boolean' then
      return false, 'Profile ' .. skin_id .. ' has an invalid enabled flag.'
    end
    if type(skin.txd_path) ~= 'string' or type(skin.dff_path) ~= 'string' then
      return false, 'Profile ' .. skin_id .. ' has an invalid asset path.'
    end
    if skin.enabled and (skin.txd_path == '' or skin.dff_path == '') then
      return false, 'Enabled profile ' .. skin_id .. ' needs TXD and DFF paths.'
    end
    if type(skin.donor_model_id) ~= 'number'
      or skin.donor_model_id % 1 ~= 0
      or skin.donor_model_id < 0
      or skin.donor_model_id >= 20000 then
      return false, 'Profile ' .. skin_id .. ' has an invalid donor model ID.'
    end
  end

  for player_name, assignment in pairs(state.config.players) do
    if player_name == '' then
      return false, 'An assignment has an empty player name.'
    end
    if type(assignment) ~= 'table' or type(assignment.enabled) ~= 'boolean' then
      return false, 'Player ' .. player_name .. ' has an invalid enabled flag.'
    end
    if type(assignment.skin_id) ~= 'string' or not state.config.skins[assignment.skin_id] then
      return false, 'Player ' .. player_name .. ' has no valid profile assignment.'
    end
  end

  return true
end

local function load_config()
  local file = io.open(CONFIG_PATH, 'rb')
  if not file then
    state.config = { skins = {}, players = {} }
    state.dirty = false
    state.selected_skin = nil
    state.selected_player = nil
    set_buffer(state.profile_id, '')
    set_buffer(state.txd_path, '')
    set_buffer(state.dff_path, '')
    state.donor_model_id[0] = 7
    state.profile_enabled[0] = true
    set_buffer(state.player_name, '')
    set_buffer(state.player_skin_id, '')
    state.player_enabled[0] = true
    set_buffer(state.profile_search, '')
    set_status('No config file yet. Saving will create it.', false)
    return true
  end

  local contents = file:read('*a')
  file:close()
  local ok, decoded = pcall(decodeJson, contents)
  if not ok or type(decoded) ~= 'table' then
    set_status('Could not parse custom_skin_loader.json. Your active file was not changed.', true)
    return false
  end

  local previous_config = state.config
  state.config = ensure_schema(decoded)
  local valid, validation_error = validate_config()
  if not valid then
    state.config = previous_config
    set_status('Could not load custom_skin_loader.json: ' .. validation_error, true)
    return false
  end
  state.dirty = false
  state.selected_skin = nil
  state.selected_player = nil
  set_buffer(state.profile_id, '')
  set_buffer(state.txd_path, '')
  set_buffer(state.dff_path, '')
  state.donor_model_id[0] = 7
  state.profile_enabled[0] = true
  set_buffer(state.player_name, '')
  set_buffer(state.player_skin_id, '')
  state.player_enabled[0] = true
  set_buffer(state.profile_search, '')
  set_status('Loaded custom_skin_loader.json.', false)
  return true
end

local function save_config()
  local valid, validation_error = validate_config()
  if not valid then
    set_status(validation_error .. ' Fix it before saving.', true)
    return false
  end

  local ok, json = pcall(encodeJson, state.config)
  if not ok then
    set_status('Could not encode the configuration as JSON.', true)
    return false
  end

  os.remove(TEMP_CONFIG_PATH)
  local file, error_message = io.open(TEMP_CONFIG_PATH, 'wb')
  if not file then
    set_status('Could not create temporary config: ' .. tostring(error_message), true)
    return false
  end

  local wrote, write_error = file:write(json)
  file:close()
  if not wrote then
    os.remove(TEMP_CONFIG_PATH)
    set_status('Could not write temporary config: ' .. tostring(write_error), true)
    return false
  end

  local replaced = kernel32.MoveFileExA(
    TEMP_CONFIG_PATH,
    CONFIG_PATH,
    MOVEFILE_REPLACE_EXISTING + MOVEFILE_WRITE_THROUGH
  )
  if replaced == 0 then
    os.remove(TEMP_CONFIG_PATH)
    set_status('Could not replace custom_skin_loader.json.', true)
    return false
  end

  state.dirty = false
  set_status('Saved. The Rust loader will reload it within one second.', false)
  return true
end

local function clear_profile_editor()
  state.selected_skin = nil
  set_buffer(state.profile_id, '')
  set_buffer(state.txd_path, '')
  set_buffer(state.dff_path, '')
  state.donor_model_id[0] = 7
  state.profile_enabled[0] = true
end

local function select_profile(skin_id)
  local skin = state.config.skins[skin_id]
  if not skin then return end

  state.selected_skin = skin_id
  set_buffer(state.profile_id, skin_id)
  set_buffer(state.txd_path, skin.txd_path)
  set_buffer(state.dff_path, skin.dff_path)
  state.donor_model_id[0] = tonumber(skin.donor_model_id) or 7
  state.profile_enabled[0] = skin.enabled
end

local function add_profile()
  local skin_id = 'new_profile'
  local suffix = 2
  while state.config.skins[skin_id] do
    skin_id = 'new_profile_' .. suffix
    suffix = suffix + 1
  end

  state.config.skins[skin_id] = {
    enabled = true,
    txd_path = '',
    dff_path = '',
    donor_model_id = 7,
  }
  select_profile(skin_id)
  state.dirty = true
  set_status('Added a draft profile. Changes are staged until Save JSON.', false)
end

local function sync_profile_id()
  local old_skin_id = state.selected_skin
  if not old_skin_id or not state.config.skins[old_skin_id] then return end

  local new_skin_id = trim(buffer_value(state.profile_id))
  if new_skin_id == '' then
    set_status('Profile ID cannot be empty. The existing profile was kept.', true)
    return
  end
  if new_skin_id == old_skin_id then return end
  if state.config.skins[new_skin_id] then
    set_buffer(state.profile_id, old_skin_id)
    set_status('A profile with that ID already exists.', true)
    return
  end

  state.config.skins[new_skin_id] = state.config.skins[old_skin_id]
  state.config.skins[old_skin_id] = nil
  for player_name, assigned_skin in pairs(state.config.players) do
    if assigned_skin.skin_id == old_skin_id then
      assigned_skin.skin_id = new_skin_id
    end
  end
  state.selected_skin = new_skin_id
  set_buffer(state.profile_id, new_skin_id)
  state.dirty = true
  set_status('Profile changes are staged. Save JSON to apply them in-game.', false)
end

local function sync_profile_fields()
  local skin_id = state.selected_skin
  local skin = skin_id and state.config.skins[skin_id]
  if not skin then return end

  skin.txd_path = buffer_value(state.txd_path)
  skin.dff_path = buffer_value(state.dff_path)
  skin.donor_model_id = state.donor_model_id[0]
  skin.enabled = state.profile_enabled[0]
  state.dirty = true
  set_status('Profile changes are staged. Save JSON to apply them in-game.', false)
end

local function delete_selected_profile()
  local skin_id = state.selected_skin
  if not skin_id or not state.config.skins[skin_id] then
    set_status('Select a profile to delete.', true)
    return
  end

  state.config.skins[skin_id] = nil
  for player_name, assigned_skin in pairs(state.config.players) do
    if assigned_skin.skin_id == skin_id then
      state.config.players[player_name] = nil
    end
  end
  clear_profile_editor()
  state.selected_player = nil
  set_buffer(state.player_name, '')
  set_buffer(state.player_skin_id, '')
  state.player_enabled[0] = true
  set_buffer(state.profile_search, '')
  state.dirty = true
  set_status('Deleted the profile and its player assignments. Save JSON to apply.', false)
end

local function clear_player_editor()
  state.selected_player = nil
  set_buffer(state.player_name, '')
  set_buffer(state.player_skin_id, '')
  state.player_enabled[0] = true
  set_buffer(state.profile_search, '')
end

local function select_player(player_name)
  local assignment = state.config.players[player_name]
  if not assignment then return end

  state.selected_player = player_name
  set_buffer(state.player_name, player_name)
  set_buffer(state.player_skin_id, assignment.skin_id)
  state.player_enabled[0] = assignment.enabled
  set_buffer(state.profile_search, '')
end

local function add_player_assignment()
  local skin_id
  for _, candidate in ipairs(sorted_keys(state.config.skins)) do
    if state.config.skins[candidate].enabled then
      skin_id = candidate
      break
    end
  end
  if not skin_id then
    set_status('Enable a profile before creating a player assignment.', true)
    return
  end

  local player_name = 'new_player'
  local suffix = 2
  while state.config.players[player_name] do
    player_name = 'new_player_' .. suffix
    suffix = suffix + 1
  end

  state.config.players[player_name] = { skin_id = skin_id, enabled = true }
  select_player(player_name)
  state.dirty = true
  set_status('Added a player assignment. Changes are staged until Save JSON.', false)
end

local function sync_player_name()
  local old_player_name = state.selected_player
  if not old_player_name or not state.config.players[old_player_name] then return end

  local new_player_name = trim(buffer_value(state.player_name))
  if new_player_name == '' then
    set_status('Player name cannot be empty. The existing assignment was kept.', true)
    return
  end
  if new_player_name == old_player_name then return end
  if state.config.players[new_player_name] then
    set_buffer(state.player_name, old_player_name)
    set_status('That player already has an assignment.', true)
    return
  end

  state.config.players[new_player_name] = state.config.players[old_player_name]
  state.config.players[old_player_name] = nil
  state.selected_player = new_player_name
  set_buffer(state.player_name, new_player_name)
  state.dirty = true
  set_status('Player assignment is staged. Save JSON to apply it in-game.', false)
end

local function sync_player_profile()
  local player_name = state.selected_player
  local skin_id = trim(buffer_value(state.player_skin_id))
  if not player_name or not state.config.players[player_name] then return end
  if not state.config.skins[skin_id] then
    set_status('Choose an existing profile ID for this player.', true)
    return
  end

  state.config.players[player_name].skin_id = skin_id
  state.dirty = true
  set_status('Player assignment is staged. Save JSON to apply it in-game.', false)
end

local function sync_player_enabled()
  local player_name = state.selected_player
  local assignment = player_name and state.config.players[player_name]
  if not assignment then return end

  assignment.enabled = state.player_enabled[0]
  state.dirty = true
  set_status('Player assignment is staged. Save JSON to apply it in-game.', false)
end

local function delete_selected_player()
  local player_name = state.selected_player
  if not player_name or not state.config.players[player_name] then
    set_status('Select a player assignment to delete.', true)
    return
  end

  state.config.players[player_name] = nil
  clear_player_editor()
  state.dirty = true
  set_status('Deleted the player assignment. Save JSON to apply it in-game.', false)
end

local function draw_profiles()
  imgui.BeginGroup()
  imgui.BeginChild('##skin_profiles', imgui.ImVec2(220, 195), true, imgui.WindowFlags.None)
    imgui.Text('Skin profiles')
    imgui.Separator()
    for _, skin_id in ipairs(sorted_keys(state.config.skins)) do
      local skin = state.config.skins[skin_id]
      local label = skin.enabled and skin_id or skin_id .. ' (disabled)'
      if imgui.Selectable(label, state.selected_skin == skin_id) then
        select_profile(skin_id)
      end
    end
  imgui.EndChild()
  if imgui.Button('Add##profile', imgui.ImVec2(106, 0)) then add_profile() end
  imgui.SameLine()
  if imgui.Button('Delete##profile', imgui.ImVec2(106, 0)) then delete_selected_profile() end
  imgui.EndGroup()

  imgui.SameLine()
  imgui.BeginGroup()
  imgui.Text(state.selected_skin and 'Edit profile' or 'Add a profile to edit it')
  imgui.PushItemWidth(320)
  if input_text('Profile ID', state.profile_id) then sync_profile_id() end
  if input_text('TXD path', state.txd_path) then sync_profile_fields() end
  if input_text('DFF path', state.dff_path) then sync_profile_fields() end
  if input_int('Donor model ID', state.donor_model_id) then sync_profile_fields() end
  if imgui.Checkbox('Enabled##profile', state.profile_enabled) then sync_profile_fields() end
  imgui.PopItemWidth()
  imgui.EndGroup()
end

local function profile_picker()
  local selected_skin_id = buffer_value(state.player_skin_id)
  local preview = selected_skin_id ~= '' and selected_skin_id or 'Choose a profile...'

  if imgui.BeginCombo('Profile ID##player', preview) then
    if imgui.IsWindowAppearing() then
      set_buffer(state.profile_search, '')
      imgui.SetKeyboardFocusHere()
    end
    input_text_with_hint('##profile_search', 'Search profiles...', state.profile_search)
    imgui.Separator()

    local search = buffer_value(state.profile_search):lower()
    local found_match = false
    for _, skin_id in ipairs(sorted_keys(state.config.skins)) do
      if search == '' or skin_id:lower():find(search, 1, true) then
        found_match = true
        local is_selected = selected_skin_id == skin_id
        local skin = state.config.skins[skin_id]
        local label = skin.enabled and skin_id or skin_id .. ' (disabled)'
        if imgui.Selectable(label, is_selected) then
          set_buffer(state.player_skin_id, skin_id)
          set_buffer(state.profile_search, '')
          sync_player_profile()
        end
      end
    end

    if not found_match then
      imgui.TextDisabled('No matching profiles.')
    end
    imgui.EndCombo()
  end
end

local function draw_players()
  imgui.BeginGroup()
  imgui.BeginChild('##player_assignments', imgui.ImVec2(220, 170), true, imgui.WindowFlags.None)
    imgui.Text('Player assignments')
    imgui.Separator()
    for _, player_name in ipairs(sorted_keys(state.config.players)) do
      local assignment = state.config.players[player_name]
      local label = player_name .. ' -> ' .. assignment.skin_id
      if not assignment.enabled then label = label .. ' (disabled)' end
      if imgui.Selectable(label, state.selected_player == player_name) then
        select_player(player_name)
      end
    end
  imgui.EndChild()
  if imgui.Button('Add##assignment', imgui.ImVec2(106, 0)) then add_player_assignment() end
  imgui.SameLine()
  if imgui.Button('Delete##assignment', imgui.ImVec2(106, 0)) then delete_selected_player() end
  imgui.EndGroup()

  imgui.SameLine()
  imgui.BeginGroup()
  imgui.Text(state.selected_player and 'Edit assignment' or 'Add an assignment to edit it')
  imgui.PushItemWidth(320)
  if input_text('Player name', state.player_name) then sync_player_name() end
  profile_picker()
  imgui.PopItemWidth()
  if imgui.Checkbox('Enabled##assignment', state.player_enabled) then sync_player_enabled() end
  imgui.EndGroup()
end

imgui.OnFrame(
  function()
    return state.window_open[0] and isSampAvailable()
  end,
  function()
    imgui.SetNextWindowSize(imgui.ImVec2(620, 560), imgui.Cond.FirstUseEver)
    imgui.Begin('Custom Skin Loader', state.window_open, imgui.WindowFlags.None)

    imgui.Text('Edit the loader configuration. Changes affect the game only after Save JSON.')
    if state.dirty then
      imgui.SameLine()
      imgui.TextColored(imgui.ImVec4(1.0, 0.8, 0.2, 1.0), 'Unsaved changes')
    end
    imgui.Separator()
    draw_profiles()
    imgui.Separator()
    draw_players()
    imgui.Separator()

    if state.status_is_error then
      imgui.TextColored(imgui.ImVec4(1.0, 0.35, 0.35, 1.0), state.status)
    else
      imgui.TextColored(imgui.ImVec4(0.45, 0.9, 0.45, 1.0), state.status)
    end

    if imgui.Button('Save JSON') then save_config() end
    imgui.SameLine()
    if imgui.Button('Reload from disk') then load_config() end
    imgui.SameLine()
    if imgui.Button('Close') then state.window_open[0] = false end
    imgui.End()
  end
)

function main()
  while not isSampAvailable() do wait(0) end
  load_config()

  sampRegisterChatCommand('skins', function()
    state.window_open[0] = not state.window_open[0]
  end)

  wait(-1)
end
