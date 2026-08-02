script_name('Wardrobe UI')
script_author('Akionka')
script_version('0.1.0')
script_properties('work-in-pause')

local ffi = require 'ffi'
local imgui = require 'mimgui'
local new = imgui.new

local CONFIG_PATH = getGameDirectory() .. [[\wardrobe.json]]
local TEMP_CONFIG_PATH = CONFIG_PATH .. '.tmp'
-- Keep this in sync with src/model_ids.rs.
local MODEL_ID_LIMIT = 20000
local MOVEFILE_REPLACE_EXISTING = 0x1
local MOVEFILE_WRITE_THROUGH = 0x8
local ONLINE_PLAYER_REFRESH_SECONDS = 1

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
  config = { skins = {}, rules = {}, presets = {} },
  window_open = new.bool(false),
  dirty = false,
  status = 'Use /wardrobe to open this editor.',
  status_is_error = false,
  selected_skin = nil,
  selected_rule = nil,
  profile_id = new.char[64](),
  txd_path = new.char[260](),
  dff_path = new.char[260](),
  donor_model_id = new.int(7),
  profile_enabled = new.bool(true),
  rule_player_name = new.char[64](),
  rule_profile_id = new.char[64](),
  rule_server_model_id = new.int(-1),
  rule_enabled = new.bool(true),
  profile_search = new.char[64](),
  preset_name = new.char[64](),
  online_players = {},
  online_players_checked_at = nil,
  selected_preset = nil,
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

local function rule_preset_key(rule)
  local player_name = rule.player_name or ''
  local server_model_id = rule.server_model_id
  return player_name .. '\31' .. (server_model_id ~= nil and tostring(server_model_id) or '')
end

local function online_players()
  local now = os.clock()
  if state.online_players_checked_at
      and now - state.online_players_checked_at < ONLINE_PLAYER_REFRESH_SECONDS then
    return state.online_players
  end

  state.online_players_checked_at = now
  state.online_players = {}
  if not isSampAvailable() then return state.online_players end

  local maximum_player_id = sampGetMaxPlayerId(false)
  if type(maximum_player_id) ~= 'number' then return state.online_players end

  for player_id = 0, maximum_player_id do
    if sampIsPlayerConnected(player_id) then
      local ok, nickname = pcall(sampGetPlayerNickname, player_id)
      if ok and type(nickname) == 'string' and nickname ~= '' then
        table.insert(state.online_players, {
          id = player_id,
          nickname = nickname,
          is_npc = sampIsPlayerNpc(player_id),
        })
      end
    end
  end

  table.sort(state.online_players, function(left, right)
    if left.is_npc ~= right.is_npc then return not left.is_npc end
    return left.id < right.id
  end)
  return state.online_players
end

local function set_status(message, is_error)
  state.status = message
  state.status_is_error = is_error or false
end

local function ensure_schema(config)
  if type(config) ~= 'table' then config = {} end
  if type(config.skins) ~= 'table' then config.skins = {} end
  if type(config.rules) ~= 'table' then config.rules = {} end
  if type(config.presets) ~= 'table' then config.presets = {} end
  config.players = nil

  for preset_id, preset in pairs(config.presets) do
    if type(preset) ~= 'table' then preset = {} end
    if type(preset.profiles) ~= 'table' then preset.profiles = {} end
    if type(preset.rules) ~= 'table' then preset.rules = {} end
    config.presets[preset_id] = preset
  end

  for _, skin in pairs(config.skins) do
    if type(skin) == 'table' and skin.enabled == nil then
      skin.enabled = true
    end
  end
  for _, rule in ipairs(config.rules) do
    if type(rule) == 'table' and rule.enabled == nil then
      rule.enabled = true
    end
  end

  return config
end

local function validate_config()
  for skin_id, skin in pairs(state.config.skins) do
    if skin_id == '' then
      return false, 'A custom skin has an empty name.'
    end
    if type(skin) ~= 'table' or type(skin.enabled) ~= 'boolean' then
      return false, 'Custom skin ' .. skin_id .. ' has an invalid enabled flag.'
    end
    if type(skin.txd_path) ~= 'string' or type(skin.dff_path) ~= 'string' then
      return false, 'Custom skin ' .. skin_id .. ' has an invalid asset path.'
    end
    if skin.enabled and (skin.txd_path == '' or skin.dff_path == '') then
      return false, 'Enabled custom skin ' .. skin_id .. ' needs TXD and DFF paths.'
    end
    if type(skin.donor_model_id) ~= 'number'
        or skin.donor_model_id % 1 ~= 0
        or skin.donor_model_id < 0
        or skin.donor_model_id >= MODEL_ID_LIMIT then
        return false,
          'Custom skin ' .. skin_id .. ' needs a GTA model donor ID from 0 to ' .. (MODEL_ID_LIMIT - 1) .. '.'
    end
  end

  for index, rule in ipairs(state.config.rules) do
    if type(rule) ~= 'table' or type(rule.enabled) ~= 'boolean' then
      return false, 'Rule ' .. index .. ' has an invalid enabled flag.'
    end
    if type(rule.profile_id) ~= 'string' or not state.config.skins[rule.profile_id] then
      return false, 'Rule ' .. index .. ' has no valid custom skin assignment.'
    end
    if rule.player_name ~= nil and (type(rule.player_name) ~= 'string' or rule.player_name == '') then
      return false, 'Rule ' .. index .. ' has an invalid player name.'
    end
    if rule.server_model_id ~= nil
        and (type(rule.server_model_id) ~= 'number'
          or rule.server_model_id % 1 ~= 0
          or rule.server_model_id < 0
          or rule.server_model_id >= 20000) then
      return false, 'Rule ' .. index .. ' has an invalid server model ID.'
    end
    if rule.player_name == nil and rule.server_model_id == nil then
      return false, 'Rule ' .. index .. ' needs a player name or server model ID.'
    end
    for previous_index = 1, index - 1 do
      local previous = state.config.rules[previous_index]
      if previous.player_name == rule.player_name
          and previous.server_model_id == rule.server_model_id then
        return false, 'Rule ' .. index .. ' duplicates rule ' .. previous_index .. '.'
      end
    end
  end

  return true
end

local function load_config()
  local file = io.open(CONFIG_PATH, 'rb')
  if not file then
    state.config = { skins = {}, rules = {}, presets = {} }
    state.dirty = false
    state.selected_skin = nil
    state.selected_rule = nil
    set_buffer(state.profile_id, '')
    set_buffer(state.txd_path, '')
    set_buffer(state.dff_path, '')
    state.donor_model_id[0] = 7
    state.profile_enabled[0] = true
    set_buffer(state.rule_player_name, '')
    set_buffer(state.rule_profile_id, '')
    state.rule_server_model_id[0] = -1
    state.rule_enabled[0] = true
    set_buffer(state.profile_search, '')
    state.selected_preset = nil
    set_buffer(state.preset_name, '')
    set_status('No config file yet. Saving will create it.', false)
    return true
  end

  local contents = file:read('*a')
  file:close()
  local ok, decoded = pcall(decodeJson, contents)
  if not ok or type(decoded) ~= 'table' then
    set_status('Could not parse wardrobe.json. Your active file was not changed.', true)
    return false
  end

  local previous_config = state.config
  state.config = ensure_schema(decoded)
  local valid, validation_error = validate_config()
  if not valid then
    state.config = previous_config
    set_status('Could not load wardrobe.json: ' .. validation_error, true)
    return false
  end
  state.dirty = false
  state.selected_skin = nil
  state.selected_rule = nil
  set_buffer(state.profile_id, '')
  set_buffer(state.txd_path, '')
  set_buffer(state.dff_path, '')
  state.donor_model_id[0] = 7
  state.profile_enabled[0] = true
  set_buffer(state.rule_player_name, '')
  set_buffer(state.rule_profile_id, '')
  state.rule_server_model_id[0] = -1
  state.rule_enabled[0] = true
  set_buffer(state.profile_search, '')
  state.selected_preset = nil
  set_buffer(state.preset_name, '')
  set_status('Loaded wardrobe.json.', false)
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
    set_status('Could not replace wardrobe.json.', true)
    return false
  end

  state.dirty = false
  set_status('Saved. The loader will reload it within one second.', false)
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
  local skin_id = 'new_skin'
  local suffix = 2
  while state.config.skins[skin_id] do
    skin_id = 'new_skin_' .. suffix
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
  set_status('Added a draft custom skin. Changes are staged until Save JSON.', false)
end

local function sync_profile_id()
  local old_skin_id = state.selected_skin
  if not old_skin_id or not state.config.skins[old_skin_id] then return end

  local new_skin_id = trim(buffer_value(state.profile_id))
  if new_skin_id == '' then
    set_status('Skin name cannot be empty. The existing custom skin was kept.', true)
    return
  end
  if new_skin_id == old_skin_id then return end
  if state.config.skins[new_skin_id] then
    set_buffer(state.profile_id, old_skin_id)
    set_status('A custom skin with that name already exists.', true)
    return
  end

  state.config.skins[new_skin_id] = state.config.skins[old_skin_id]
  state.config.skins[old_skin_id] = nil
  for _, rule in ipairs(state.config.rules) do
    if rule.profile_id == old_skin_id then
      rule.profile_id = new_skin_id
    end
  end
  for _, preset in pairs(state.config.presets) do
    if preset.profiles[old_skin_id] ~= nil then
      preset.profiles[new_skin_id] = preset.profiles[old_skin_id]
      preset.profiles[old_skin_id] = nil
    end
  end
  state.selected_skin = new_skin_id
  set_buffer(state.profile_id, new_skin_id)
  state.dirty = true
  set_status('Custom skin changes are staged. Save JSON to apply them in-game.', false)
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
  set_status('Custom skin changes are staged. Save JSON to apply them in-game.', false)
end

local function delete_selected_profile()
  local skin_id = state.selected_skin
  if not skin_id or not state.config.skins[skin_id] then
    set_status('Select a custom skin to delete.', true)
    return
  end

  state.config.skins[skin_id] = nil
  for _, preset in pairs(state.config.presets) do
    preset.profiles[skin_id] = nil
  end
  for index = #state.config.rules, 1, -1 do
    if state.config.rules[index].profile_id == skin_id then
      local removed_rule = state.config.rules[index]
      local removed_rule_key = rule_preset_key(removed_rule)
      for _, preset in pairs(state.config.presets) do
        preset.rules[removed_rule_key] = nil
      end
      table.remove(state.config.rules, index)
    end
  end
  clear_profile_editor()
  state.selected_rule = nil
  set_buffer(state.rule_player_name, '')
  set_buffer(state.rule_profile_id, '')
  state.rule_server_model_id[0] = -1
  state.rule_enabled[0] = true
  set_buffer(state.profile_search, '')
  state.dirty = true
  set_status('Deleted the custom skin and its matching rules. Save JSON to apply.', false)
end

local function clear_rule_editor()
  state.selected_rule = nil
  set_buffer(state.rule_player_name, '')
  set_buffer(state.rule_profile_id, '')
  state.rule_server_model_id[0] = -1
  state.rule_enabled[0] = true
  set_buffer(state.profile_search, '')
end

local function select_rule(index)
  local rule = state.config.rules[index]
  if not rule then return end

  state.selected_rule = index
  set_buffer(state.rule_player_name, rule.player_name or '')
  set_buffer(state.rule_profile_id, rule.profile_id)
  state.rule_server_model_id[0] = rule.server_model_id or -1
  state.rule_enabled[0] = rule.enabled
  set_buffer(state.profile_search, '')
end

local function add_rule()
  local profile_ids = sorted_keys(state.config.skins)
  if #profile_ids == 0 then
    set_status('Add a custom skin before creating a matching rule.', true)
    return
  end

  table.insert(state.config.rules, {
    profile_id = profile_ids[1],
    enabled = true,
  })
  select_rule(#state.config.rules)
  state.dirty = true
  set_status('Added a rule. Set a player name or server model ID before saving.', false)
end

local function sync_rule_profile()
  local index = state.selected_rule
  local rule = index and state.config.rules[index]
  local profile_id = trim(buffer_value(state.rule_profile_id))
  if not rule then return end
  if not state.config.skins[profile_id] then
    set_status('Choose an existing custom skin for this rule.', true)
    return
  end

  rule.profile_id = profile_id
  state.dirty = true
  set_status('Rule changes are staged. Save JSON to apply them in-game.', false)
end

local function sync_rule_conditions()
  local index = state.selected_rule
  local rule = index and state.config.rules[index]
  if not rule then return end

  local previous_key = rule_preset_key(rule)
  local player_name = trim(buffer_value(state.rule_player_name))
  local server_model_id = state.rule_server_model_id[0]
  rule.player_name = player_name ~= '' and player_name or nil
  rule.server_model_id = server_model_id >= 0 and server_model_id or nil
  rule.enabled = state.rule_enabled[0]
  local current_key = rule_preset_key(rule)
  if current_key ~= previous_key then
    for _, preset in pairs(state.config.presets) do
      if preset.rules[previous_key] ~= nil then
        preset.rules[current_key] = preset.rules[previous_key]
        preset.rules[previous_key] = nil
      end
    end
  end
  state.dirty = true
  set_status('Rule changes are staged. Save JSON to apply them in-game.', false)
end

local function delete_selected_rule()
  local index = state.selected_rule
  if not index or not state.config.rules[index] then
    set_status('Select a matching rule to delete.', true)
    return
  end

  local removed_rule_key = rule_preset_key(state.config.rules[index])
  for _, preset in pairs(state.config.presets) do
    preset.rules[removed_rule_key] = nil
  end
  table.remove(state.config.rules, index)
  clear_rule_editor()
  state.dirty = true
  set_status('Deleted the matching rule. Save JSON to apply.', false)
end

local function clear_preset_editor()
  state.selected_preset = nil
  set_buffer(state.preset_name, '')
end

local function select_preset(preset_id)
  if not state.config.presets[preset_id] then return end
  state.selected_preset = preset_id
  set_buffer(state.preset_name, preset_id)
end

local function preset_activation_state()
  local profiles = {}
  for skin_id, skin in pairs(state.config.skins) do
    profiles[skin_id] = skin.enabled == true
  end
  local rules = {}
  for _, rule in ipairs(state.config.rules) do
    rules[rule_preset_key(rule)] = rule.enabled == true
  end
  return { profiles = profiles, rules = rules }
end

local function add_preset()
  local preset_id = 'new_preset'
  local suffix = 2
  while state.config.presets[preset_id] do
    preset_id = 'new_preset_' .. suffix
    suffix = suffix + 1
  end

  state.config.presets[preset_id] = preset_activation_state()
  state.selected_preset = preset_id
  set_buffer(state.preset_name, preset_id)
  state.dirty = true
  set_status('Added preset ' .. preset_id .. '. Save JSON to keep it.', false)
end

local function sync_preset_name()
  local old_preset_id = state.selected_preset
  if not old_preset_id or not state.config.presets[old_preset_id] then return end

  local new_preset_id = trim(buffer_value(state.preset_name))
  if new_preset_id == '' then
    set_buffer(state.preset_name, old_preset_id)
    set_status('Preset name cannot be empty.', true)
    return
  end
  if new_preset_id == old_preset_id then return end
  if state.config.presets[new_preset_id] then
    set_buffer(state.preset_name, old_preset_id)
    set_status('A preset with that name already exists.', true)
    return
  end

  state.config.presets[new_preset_id] = state.config.presets[old_preset_id]
  state.config.presets[old_preset_id] = nil
  state.selected_preset = new_preset_id
  set_buffer(state.preset_name, new_preset_id)
  state.dirty = true
  set_status('Preset changes are staged. Save JSON to keep them.', false)
end

local function apply_selected_preset()
  local preset_id = state.selected_preset
  local preset = preset_id and state.config.presets[preset_id]
  if not preset then
    set_status('Select a preset to apply.', true)
    return
  end

  for skin_id, skin in pairs(state.config.skins) do
    skin.enabled = preset.profiles[skin_id] == true
  end
  for _, rule in ipairs(state.config.rules) do
    rule.enabled = preset.rules[rule_preset_key(rule)] == true
  end
  if state.selected_skin then select_profile(state.selected_skin) end
  if state.selected_rule then select_rule(state.selected_rule) end
  state.dirty = true
  set_status('Applied preset ' .. preset_id .. '. Save JSON to apply it in-game.', false)
end

local function sync_selected_preset_skin(skin_id)
  local preset_id = state.selected_preset
  local preset = preset_id and state.config.presets[preset_id]
  local skin = skin_id and state.config.skins[skin_id]
  if not preset or not skin then return end

  preset.profiles[skin_id] = skin.enabled == true
  state.dirty = true
  set_status('Updated preset ' .. preset_id .. '. Save JSON to keep it.', false)
end

local function sync_selected_preset_rule(index)
  local preset_id = state.selected_preset
  local preset = preset_id and state.config.presets[preset_id]
  local rule = index and state.config.rules[index]
  if not preset or not rule then return end

  preset.rules[rule_preset_key(rule)] = rule.enabled == true
  state.dirty = true
  set_status('Updated preset ' .. preset_id .. '. Save JSON to keep it.', false)
end

local function delete_selected_preset()
  local preset_id = state.selected_preset
  if not preset_id or not state.config.presets[preset_id] then
    set_status('Select a preset to delete.', true)
    return
  end

  state.config.presets[preset_id] = nil
  clear_preset_editor()
  state.dirty = true
  set_status('Deleted the preset. Save JSON to keep the change.', false)
end

local function draw_profiles()
  imgui.BeginGroup()
  imgui.BeginChild('##skin_profiles', imgui.ImVec2(220, 195), true, imgui.WindowFlags.None)
  imgui.Text('Custom skins')
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
  imgui.Text(state.selected_skin and 'Edit custom skin' or 'Add a custom skin to edit it')
  imgui.PushItemWidth(320)
  if input_text('Skin name', state.profile_id) then sync_profile_id() end
  if input_text('TXD path', state.txd_path) then sync_profile_fields() end
  if input_text('DFF path', state.dff_path) then sync_profile_fields() end
  if input_int('Donor model ID', state.donor_model_id) then sync_profile_fields() end
  if imgui.Checkbox('Enabled##profile', state.profile_enabled) then
    sync_profile_fields()
    sync_selected_preset_skin(state.selected_skin)
  end
  imgui.PopItemWidth()
  imgui.EndGroup()
end

local function draw_presets()
  imgui.BeginGroup()
  imgui.BeginChild('##activation_presets', imgui.ImVec2(220, 120), true, imgui.WindowFlags.None)
  imgui.Text('Activation presets')
  imgui.Separator()
  for _, preset_id in ipairs(sorted_keys(state.config.presets)) do
    if imgui.Selectable(preset_id .. '##preset', state.selected_preset == preset_id) then
      select_preset(preset_id)
      apply_selected_preset()
    end
  end
  imgui.EndChild()
  if imgui.Button('New##preset', imgui.ImVec2(106, 0)) then add_preset() end
  imgui.SameLine()
  if imgui.Button('Delete##preset', imgui.ImVec2(106, 0)) then delete_selected_preset() end
  imgui.EndGroup()

  imgui.SameLine()
  imgui.BeginGroup()
  imgui.Text(state.selected_preset and 'Edit activation preset' or 'Add an activation preset')
  imgui.PushItemWidth(320)
  if input_text('Preset name', state.preset_name) then sync_preset_name() end
  imgui.PopItemWidth()
  imgui.TextDisabled('Click a preset to apply it. Toggle changes update the selected preset.')
  imgui.EndGroup()
end

local function rule_profile_picker()
  local selected_profile_id = buffer_value(state.rule_profile_id)
  local preview = selected_profile_id ~= '' and selected_profile_id or 'Choose a custom skin...'

  if imgui.BeginCombo('Custom skin##rule', preview) then
    if imgui.IsWindowAppearing() then
      set_buffer(state.profile_search, '')
      imgui.SetKeyboardFocusHere()
    end
    input_text_with_hint('##rule_profile_search', 'Search custom skins...', state.profile_search)
    imgui.Separator()

    local search = buffer_value(state.profile_search):lower()
    local found_match = false
    for _, skin_id in ipairs(sorted_keys(state.config.skins)) do
      if search == '' or skin_id:lower():find(search, 1, true) then
        found_match = true
        local is_selected = selected_profile_id == skin_id
        local skin = state.config.skins[skin_id]
        local label = skin.enabled and skin_id or skin_id .. ' (disabled)'
        if imgui.Selectable(label, is_selected) then
          set_buffer(state.rule_profile_id, skin_id)
          set_buffer(state.profile_search, '')
          sync_rule_profile()
        end
      end
    end

    if not found_match then
      imgui.TextDisabled('No matching custom skins.')
    end
    imgui.EndCombo()
  end
end

local function rule_player_name_selector()
  local selected_name = buffer_value(state.rule_player_name)
  local preview = selected_name ~= '' and selected_name or '<Any player>'

  if imgui.BeginCombo('Player name##rule', preview) then
    if imgui.IsWindowAppearing() then
      imgui.SetKeyboardFocusHere()
    end
    if input_text('##rule_player_name', state.rule_player_name) then
      sync_rule_conditions()
    end
    imgui.Separator()

    local selected_name = buffer_value(state.rule_player_name)
    local search = selected_name:lower()
    if imgui.Selectable('<Any player>##empty_player_name', selected_name == '') then
      set_buffer(state.rule_player_name, '')
      sync_rule_conditions()
    end
    imgui.Separator()

    local has_players = false
    for _, player in ipairs(online_players()) do
      if search == '' or player.nickname:lower():find(search, 1, true) then
        has_players = true
        local is_selected = selected_name == player.nickname
        local player_kind = player.is_npc and ' [NPC]' or ''
        local label = string.format(
          '[%d] %s%s##online_player_%d',
          player.id,
          player.nickname,
          player_kind,
          player.id
        )
        if imgui.Selectable(label, is_selected) then
          set_buffer(state.rule_player_name, player.nickname)
          sync_rule_conditions()
        end
      end
    end

    if not has_players then
      imgui.TextDisabled('No matching connected players.')
    end
    imgui.EndCombo()
  end
end

local function rule_label(rule)
  local condition
  if rule.player_name and rule.server_model_id ~= nil then
    condition = rule.player_name .. ' + model ' .. rule.server_model_id
  elseif rule.player_name then
    condition = rule.player_name
  elseif rule.server_model_id ~= nil then
    condition = 'model ' .. rule.server_model_id
  else
    condition = 'incomplete rule'
  end

  local label = condition .. ' -> ' .. tostring(rule.profile_id)
  if not rule.enabled then label = label .. ' (disabled)' end
  return label
end

local function rule_priority_label(rule)
  if rule.player_name and rule.server_model_id ~= nil then
    return 'Priority: player + server model'
  elseif rule.player_name then
    return 'Priority: player name'
  elseif rule.server_model_id ~= nil then
    return 'Priority: server model'
  end
  return 'Set a player name or server model ID.'
end

local function draw_rules()
  imgui.BeginGroup()
  imgui.BeginChild('##skin_rules', imgui.ImVec2(220, 170), true, imgui.WindowFlags.None)
  imgui.Text('Matching rules')
  imgui.Separator()
  for index, rule in ipairs(state.config.rules) do
    if imgui.Selectable(rule_label(rule) .. '##rule' .. index, state.selected_rule == index) then
      select_rule(index)
    end
  end
  imgui.EndChild()
  if imgui.Button('Add##rule', imgui.ImVec2(106, 0)) then add_rule() end
  imgui.SameLine()
  if imgui.Button('Delete##rule', imgui.ImVec2(106, 0)) then delete_selected_rule() end
  imgui.EndGroup()

  imgui.SameLine()
  imgui.BeginGroup()
  imgui.Text(state.selected_rule and 'Edit matching rule' or 'Add a rule to edit it')
  imgui.PushItemWidth(320)
  rule_player_name_selector()
  if input_int('Server model ID (-1 = any)', state.rule_server_model_id) then sync_rule_conditions() end
  rule_profile_picker()
  imgui.PopItemWidth()
  if imgui.Checkbox('Enabled##rule', state.rule_enabled) then
    sync_rule_conditions()
    sync_selected_preset_rule(state.selected_rule)
  end
  if state.selected_rule then
    imgui.TextDisabled(rule_priority_label(state.config.rules[state.selected_rule]))
  end
  imgui.EndGroup()
end

imgui.OnFrame(
  function()
    return state.window_open[0] and isSampAvailable()
  end,
  function()
    imgui.SetNextWindowSize(imgui.ImVec2(620, 710), imgui.Cond.FirstUseEver)
    imgui.Begin('Wardrobe', state.window_open, imgui.WindowFlags.None)

    imgui.Text('Edit the loader configuration. Changes affect the game only after Save JSON.')
    if state.dirty then
      imgui.SameLine()
      imgui.TextColored(imgui.ImVec4(1.0, 0.8, 0.2, 1.0), 'Unsaved changes')
    end
    imgui.Separator()
    draw_profiles()
    imgui.Separator()
    draw_presets()
    imgui.Separator()
    draw_rules()
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

  sampRegisterChatCommand('wardrobe', function()
    state.window_open[0] = not state.window_open[0]
  end)

  wait(-1)
end
